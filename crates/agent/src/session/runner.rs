//! The single durable writer and execution loop for one active session.

use std::sync::Arc;
use std::time::Duration;

use super::compression::SegmentCompressor;
use super::deadline::DeadlineScheduler;
use super::decision::{decide, Decision, DecisionWorld};
use super::events::{
    DeadlineKind, Input, ProviderErrorRecord, Purpose, RuntimeFailure, Selection, SessionEvent,
    ToolInvocation, ToolOutcome, ToolResultData, TurnOutcome, Usage, TOOL_CANCEL_NAME,
};
use super::executor::{CompletedTool, ToolExecutor};
use super::model::{
    ModelError, ModelGateway, ModelOutcome, ModelReleaseSuggestion, ModelRequest,
    ModelStreamObserver, TOOL_INTERRUPTED_MESSAGE,
};
use super::ports::{Clock, IdGenerator};
use super::projection::provider_transcript;
use super::state::{snapshot_value, SessionState, STATE_SCHEMA_VERSION};
use super::store::{EventEnvelope, SessionStore, StoreError};
use super::tools::{provider_call_definition, DynamicCall, ToolExecution, ToolRegistry};

pub type InputBudget = Arc<dyn Fn(&SessionState) -> Option<u64> + Send + Sync>;
pub type MaxOutputTokens = Arc<dyn Fn(&SessionState) -> Option<u32> + Send + Sync>;

pub trait RunnerObserver: Send + Sync {
    fn persisted(&self, _session_id: &str, _events: &[EventEnvelope]) {}
    fn text_delta(&self, _session_id: &str, _generation: u64, _step_id: &str, _text: &str) {}
}

pub struct NoopRunnerObserver;

impl RunnerObserver for NoopRunnerObserver {}

#[derive(Clone)]
pub struct RunnerOptions {
    pub auto_wait: Duration,
    pub handoff_timeout: Duration,
    pub provider_retry_limit: u32,
    pub provider_retry_base: Duration,
    pub provider_retry_max: Duration,
    pub tool_result_capacity: usize,
    pub max_tool_result_json_bytes: usize,
    pub max_tool_result_message_bytes: usize,
    pub input_budget: InputBudget,
    pub max_output_tokens: MaxOutputTokens,
}

impl Default for RunnerOptions {
    fn default() -> Self {
        Self {
            auto_wait: Duration::from_secs(60),
            handoff_timeout: Duration::from_secs(10 * 60),
            provider_retry_limit: 10,
            provider_retry_base: Duration::from_millis(500),
            provider_retry_max: Duration::from_secs(5),
            tool_result_capacity: 64,
            max_tool_result_json_bytes: 1024 * 1024,
            max_tool_result_message_bytes: 64 * 1024,
            input_budget: Arc::new(|_| None),
            max_output_tokens: Arc::new(|_| None),
        }
    }
}

#[derive(Clone)]
pub struct RunnerDependencies {
    pub store: Arc<dyn SessionStore>,
    pub model: Arc<dyn ModelGateway>,
    pub tools: Arc<ToolRegistry>,
    pub executor: Arc<ToolExecutor>,
    pub deadlines: DeadlineScheduler,
    pub clock: Arc<dyn Clock>,
    pub ids: Arc<dyn IdGenerator>,
    pub observer: Arc<dyn RunnerObserver>,
    pub compressor: SegmentCompressor,
    pub options: RunnerOptions,
}

pub enum RunnerCommand {
    Create {
        selection: Selection,
        system_prompt: Option<String>,
        workspace: String,
        response: Response,
        capacity: Option<tokio::sync::OwnedSemaphorePermit>,
    },
    Input {
        content: String,
        response: Response,
        capacity: Option<tokio::sync::OwnedSemaphorePermit>,
    },
    SetSelection {
        selection: Selection,
        response: Response,
        capacity: Option<tokio::sync::OwnedSemaphorePermit>,
    },
    CancelTurn {
        response: Response,
        capacity: Option<tokio::sync::OwnedSemaphorePermit>,
    },
    RecordFault {
        failure: RuntimeFailure,
        consecutive_count: u32,
        circuit_open: bool,
    },
    Deadline(DeadlineKind),
    Inspect(tokio::sync::oneshot::Sender<SessionState>),
    Stop(Response),
}

pub type Response = tokio::sync::oneshot::Sender<Result<(), RunnerRequestError>>;

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("{message}")]
pub struct RunnerRequestError {
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunnerFailure {
    pub failure_id: String,
    pub stage: String,
    pub message: String,
}

impl RunnerFailure {
    pub fn fingerprint(&self) -> String {
        RuntimeFailure {
            failure_id: self.failure_id.clone(),
            stage: self.stage.clone(),
            message: self.message.clone(),
        }
        .fingerprint()
    }
}

pub struct IdleRunner {
    pub state: SessionState,
    pub commands: tokio::sync::mpsc::Receiver<RunnerCommand>,
}

pub enum RunnerExit {
    Idle(Box<IdleRunner>),
    CircuitOpen,
    Stopped,
    Failed(RunnerFailure),
}

pub struct SessionRunner {
    dependencies: RunnerDependencies,
    state: SessionState,
    commands: tokio::sync::mpsc::Receiver<RunnerCommand>,
    tool_results: tokio::sync::mpsc::Receiver<CompletedTool>,
    tool_result_sender: tokio::sync::mpsc::Sender<CompletedTool>,
}

impl SessionRunner {
    pub fn new(
        dependencies: RunnerDependencies,
        state: SessionState,
        commands: tokio::sync::mpsc::Receiver<RunnerCommand>,
    ) -> Self {
        let (tool_result_sender, tool_results) =
            tokio::sync::mpsc::channel(dependencies.options.tool_result_capacity.max(1));
        Self {
            dependencies,
            state,
            commands,
            tool_results,
            tool_result_sender,
        }
    }

    pub async fn run(mut self) -> RunnerExit {
        loop {
            while let Ok(command) = self.commands.try_recv() {
                match self.handle_command(command).await {
                    Ok(CommandEffect::Continue) | Ok(CommandEffect::CancelTurn) => {}
                    Ok(CommandEffect::CircuitOpen) => return RunnerExit::CircuitOpen,
                    Ok(CommandEffect::Stop) => return RunnerExit::Stopped,
                    Err(error) => return RunnerExit::Failed(error),
                }
            }
            while let Ok(completed) = self.tool_results.try_recv() {
                if let Err(error) = self.persist_tool_result(completed).await {
                    return RunnerExit::Failed(error);
                }
            }

            let world = self.world();
            match decide(&self.state, &world) {
                Decision::InterruptStep { step_id, reason } => {
                    if let Err(error) = self
                        .append(vec![SessionEvent::StepInterrupted {
                            step_id,
                            reason,
                            interrupted_at_ms: self.dependencies.clock.now_ms(),
                        }])
                        .await
                    {
                        return RunnerExit::Failed(error);
                    }
                }
                Decision::InterruptTools { invocation_ids } => {
                    let events = invocation_ids
                        .into_iter()
                        .filter_map(|invocation_id| {
                            self.state.pending(&invocation_id).map(|pending| {
                                SessionEvent::ToolResult {
                                    result: ToolResultData {
                                        result_id: self.dependencies.ids.next(),
                                        invocation_id,
                                        tool: pending.invocation.tool.clone(),
                                        outcome: ToolOutcome::Interrupted,
                                        message: TOOL_INTERRUPTED_MESSAGE.into(),
                                        data: serde_json::json!({
                                            "reason": "runtime_interrupted",
                                            "result_unknown": true,
                                        }),
                                        result_schema_version: 1,
                                        knowledge: None,
                                        finished_at_ms: self.dependencies.clock.now_ms(),
                                    },
                                }
                            })
                        })
                        .collect();
                    if let Err(error) = self.append(events).await {
                        return RunnerExit::Failed(error);
                    }
                }
                Decision::StartTurn => {
                    if let Err(error) = self
                        .append(vec![SessionEvent::TurnStarted {
                            turn_id: self.dependencies.ids.next(),
                            started_at_ms: self.dependencies.clock.now_ms(),
                        }])
                        .await
                    {
                        return RunnerExit::Failed(error);
                    }
                }
                Decision::EndAutoWait { step_id, reason } => {
                    if let Err(error) = self
                        .append(vec![SessionEvent::AutoWaitEnded {
                            step_id,
                            reason,
                            ended_at_ms: self.dependencies.clock.now_ms(),
                        }])
                        .await
                    {
                        return RunnerExit::Failed(error);
                    }
                }
                Decision::StartStep {
                    purpose,
                    include_pending_tools,
                    outstanding,
                } => {
                    match self
                        .start_step(purpose, include_pending_tools, outstanding)
                        .await
                    {
                        Ok(StepEffect::Continue) => {}
                        Ok(StepEffect::CircuitOpen) => return RunnerExit::CircuitOpen,
                        Ok(StepEffect::Stop) => return RunnerExit::Stopped,
                        Err(error) => return RunnerExit::Failed(error),
                    }
                }
                Decision::ApplyHandoff { document, failure } => {
                    if let Err(error) = self.apply_handoff(document, failure).await {
                        return RunnerExit::Failed(error);
                    }
                }
                Decision::FinishTurn {
                    outcome,
                    outstanding,
                } => {
                    let Some(turn) = self.state.active_turn.as_ref() else {
                        return RunnerExit::Failed(invariant_failure(
                            "decision.finish_turn",
                            "FinishTurn selected without an active turn",
                        ));
                    };
                    let turn_id = turn.turn_id.clone();
                    if let Err(error) = self
                        .append(vec![SessionEvent::TurnFinished {
                            turn_id,
                            outcome,
                            outstanding,
                            finished_at_ms: self.dependencies.clock.now_ms(),
                        }])
                        .await
                    {
                        return RunnerExit::Failed(error);
                    }
                }
                Decision::ReachDeadline { deadline } => {
                    if let Err(error) = self
                        .append(vec![SessionEvent::DeadlineReached {
                            deadline,
                            reached_at_ms: self.dependencies.clock.now_ms(),
                        }])
                        .await
                    {
                        return RunnerExit::Failed(error);
                    }
                }
                Decision::Park { .. } => {
                    if self.is_fully_idle() {
                        return RunnerExit::Idle(Box::new(IdleRunner {
                            state: self.state,
                            commands: self.commands,
                        }));
                    }
                    if let Err(error) = self.arm_current_deadline().await {
                        return RunnerExit::Failed(error);
                    }
                    tokio::select! {
                        command = self.commands.recv() => match command {
                            Some(command) => match self.handle_command(command).await {
                                Ok(CommandEffect::Continue) | Ok(CommandEffect::CancelTurn) => {}
                                Ok(CommandEffect::CircuitOpen) => return RunnerExit::CircuitOpen,
                                Ok(CommandEffect::Stop) => return RunnerExit::Stopped,
                                Err(error) => return RunnerExit::Failed(error),
                            },
                            None => return RunnerExit::Stopped,
                        },
                        completed = self.tool_results.recv() => {
                            if let Some(completed) = completed {
                                if let Err(error) = self.persist_tool_result(completed).await {
                                    return RunnerExit::Failed(error);
                                }
                            }
                        }
                    }
                }
            }
            tokio::task::yield_now().await;
        }
    }

    fn world(&self) -> DecisionWorld {
        DecisionWorld {
            now_ms: self.dependencies.clock.now_ms(),
            live_tools: self.dependencies.executor.live(&self.state.session_id),
            tool_changes: self.dependencies.tools.changes(&self.state.known_tools),
            outstanding: self.state.outstanding(&self.dependencies.tools),
            estimated_input_tokens: estimate_input_tokens(&self.state),
            input_budget: (self.dependencies.options.input_budget)(&self.state),
            provider_retry_limit: self.dependencies.options.provider_retry_limit,
            handoff_timeout_ms: duration_ms(self.dependencies.options.handoff_timeout),
        }
    }

    fn is_fully_idle(&self) -> bool {
        let cancelled_results_are_parked = self.state.last_turn_outcome
            == Some(TurnOutcome::Cancelled)
            && self
                .state
                .pending_tools
                .values()
                .all(|pending| pending.result.is_some());
        self.state.is_created()
            && self.state.active_turn.is_none()
            && self.state.active_step.is_none()
            && (self.state.pending_tools.is_empty() || cancelled_results_are_parked)
            && self.state.unconsumed_inputs.is_empty()
            && self.state.pending_notices.is_empty()
            && self.state.auto_wait.is_none()
            && self.state.wait_deadline.is_none()
    }

    async fn arm_current_deadline(&self) -> Result<(), RunnerFailure> {
        if let Some(wait) = &self.state.auto_wait {
            self.dependencies
                .deadlines
                .arm(
                    self.state.session_id.clone(),
                    DeadlineKind::AutoWait {
                        step_id: wait.step_id.clone(),
                    },
                    wait.deadline_ms,
                )
                .await
                .map_err(|error| infrastructure_failure("deadline.arm", error))?;
        }
        if let Some(wait) = &self.state.wait_deadline {
            self.dependencies
                .deadlines
                .arm(
                    self.state.session_id.clone(),
                    DeadlineKind::Wait {
                        invocation_id: wait.invocation_id.clone(),
                    },
                    wait.deadline_ms,
                )
                .await
                .map_err(|error| infrastructure_failure("deadline.arm", error))?;
        }
        Ok(())
    }

    async fn start_step(
        &mut self,
        purpose: Purpose,
        include_pending_tools: bool,
        mut outstanding: Vec<super::events::OutstandingItem>,
    ) -> Result<StepEffect, RunnerFailure> {
        if let Some(turn) = &self.state.active_turn {
            if turn.consecutive_provider_failures > 0 {
                match self
                    .wait_retry_backoff(turn.consecutive_provider_failures)
                    .await?
                {
                    CommandEffect::Continue => {}
                    CommandEffect::CancelTurn => return Ok(StepEffect::Continue),
                    CommandEffect::CircuitOpen => return Ok(StepEffect::CircuitOpen),
                    CommandEffect::Stop => return Ok(StepEffect::Stop),
                }
            }
        }

        if purpose == Purpose::Handoff && self.handoff_expired() {
            return Ok(StepEffect::Continue);
        }

        let turn_id = self
            .state
            .active_turn
            .as_ref()
            .map(|turn| turn.turn_id.clone())
            .ok_or_else(|| {
                invariant_failure(
                    "decision.start_step",
                    "StartStep selected without an active turn",
                )
            })?;
        let step_id = self.dependencies.ids.next();
        let consumed_inputs = if purpose == Purpose::Conversation {
            self.state
                .unconsumed_inputs
                .iter()
                .map(|input| input.input_id.clone())
                .collect()
        } else {
            Vec::new()
        };
        outstanding
            .retain(|item| item.kind != "mailbox_input" || !consumed_inputs.contains(&item.id));
        let mut notices = self.state.pending_notices.clone();
        if purpose == Purpose::Handoff {
            notices.push(
                "Prepare a complete successor-facing context handoff document, then call handoff. The handoff document is the only information that may be absent if this request fails."
                    .into(),
            );
        }
        let max_output_tokens = (self.dependencies.options.max_output_tokens)(&self.state);
        self.append(vec![SessionEvent::StepStarted {
            step_id: step_id.clone(),
            turn_id,
            purpose,
            consumed_inputs,
            deliveries: self.state.planned_deliveries(include_pending_tools),
            tool_changes: self.dependencies.tools.changes(&self.state.known_tools),
            notices,
            outstanding,
            max_output_tokens,
            started_at_ms: self.dependencies.clock.now_ms(),
        }])
        .await?;

        let selection = self.state.selection.clone().ok_or_else(|| {
            invariant_failure("model.request", "session has no provider selection")
        })?;
        let request = ModelRequest {
            session_id: self.state.session_id.clone(),
            generation: self.state.generation.number,
            step_id: step_id.clone(),
            selection,
            transcript: provider_transcript(&self.state),
            tools: Arc::new(vec![provider_call_definition()]),
            max_output_tokens,
            stream_observer: Arc::new(StepStreamObserver {
                observer: self.dependencies.observer.clone(),
            }),
        };
        let model = self.dependencies.model.clone();
        let mut task = tokio::spawn(async move { model.complete(&request).await });
        let handoff_deadline_ms = (purpose == Purpose::Handoff)
            .then(|| self.handoff_deadline_ms())
            .flatten();
        let clock = self.dependencies.clock.clone();
        let handoff_timeout = async move {
            match handoff_deadline_ms {
                Some(deadline_ms) => clock.sleep_until(deadline_ms).await,
                None => std::future::pending::<()>().await,
            }
        };
        tokio::pin!(handoff_timeout);

        loop {
            tokio::select! {
                joined = &mut task => {
                    let result = joined.map_err(|error| RunnerFailure {
                        failure_id: "model_task".into(),
                        stage: "model.complete".into(),
                        message: error.to_string(),
                    })?;
                    return match result {
                        Ok(outcome) => {
                            self.complete_step(step_id, outcome).await?;
                            Ok(StepEffect::Continue)
                        }
                        Err(error) => {
                            self.fail_step(step_id, error).await?;
                            Ok(StepEffect::Continue)
                        }
                    };
                }
                _ = &mut handoff_timeout => {
                    task.abort();
                    let _ = task.await;
                    self.append(vec![SessionEvent::StepInterrupted {
                        step_id,
                        reason: super::events::StepInterruptionReason::HandoffTimeout,
                        interrupted_at_ms: self.dependencies.clock.now_ms(),
                    }]).await?;
                    return Ok(StepEffect::Continue);
                }
                command = self.commands.recv() => match command {
                    Some(command) => match self.handle_command(command).await? {
                        CommandEffect::Continue => {}
                        CommandEffect::CancelTurn => {
                            task.abort();
                            let _ = task.await;
                            self.append(vec![SessionEvent::StepInterrupted {
                                step_id,
                                reason: super::events::StepInterruptionReason::TurnCancelled,
                                interrupted_at_ms: self.dependencies.clock.now_ms(),
                            }]).await?;
                            return Ok(StepEffect::Continue);
                        }
                        CommandEffect::CircuitOpen => {
                            task.abort();
                            let _ = task.await;
                            return Ok(StepEffect::CircuitOpen);
                        }
                        CommandEffect::Stop => {
                            task.abort();
                            let _ = task.await;
                            return Ok(StepEffect::Stop);
                        }
                    },
                    None => {
                        task.abort();
                        let _ = task.await;
                        return Ok(StepEffect::Stop);
                    }
                },
                completed = self.tool_results.recv() => {
                    if let Some(completed) = completed {
                        self.persist_tool_result(completed).await?;
                    }
                }
            }
        }
    }

    async fn wait_retry_backoff(&mut self, failures: u32) -> Result<CommandEffect, RunnerFailure> {
        let shift = failures.saturating_sub(1).min(31);
        let factor = 1_u32 << shift;
        let duration = self
            .dependencies
            .options
            .provider_retry_base
            .saturating_mul(factor)
            .min(self.dependencies.options.provider_retry_max);
        let sleep = self.dependencies.clock.sleep(duration);
        tokio::pin!(sleep);
        loop {
            tokio::select! {
                _ = &mut sleep => return Ok(CommandEffect::Continue),
                command = self.commands.recv() => match command {
                    Some(command) => match self.handle_command(command).await? {
                        CommandEffect::Continue => {}
                        effect => return Ok(effect),
                    },
                    None => return Ok(CommandEffect::Stop),
                },
                completed = self.tool_results.recv() => {
                    if let Some(completed) = completed {
                        self.persist_tool_result(completed).await?;
                    }
                }
            }
        }
    }

    fn handoff_deadline_ms(&self) -> Option<i64> {
        self.state
            .active_turn
            .as_ref()?
            .handoff
            .as_ref()
            .map(|progress| {
                progress
                    .started_at_ms
                    .saturating_add(duration_ms(self.dependencies.options.handoff_timeout))
            })
    }

    fn handoff_expired(&self) -> bool {
        self.handoff_deadline_ms()
            .is_some_and(|deadline_ms| self.dependencies.clock.now_ms() >= deadline_ms)
    }

    async fn complete_step(
        &mut self,
        step_id: String,
        outcome: ModelOutcome,
    ) -> Result<(), RunnerFailure> {
        let completed_at_ms = self.dependencies.clock.now_ms();
        let invocations = match parse_invocations(
            &self.state,
            &outcome,
            completed_at_ms,
            self.dependencies.ids.as_ref(),
        ) {
            Ok(invocations) => invocations,
            Err(error) => {
                return self
                    .fail_step_with_record(
                        step_id,
                        ProviderErrorRecord {
                            stage: "provider.output.tool_call".into(),
                            retryable: true,
                            status_code: None,
                            provider_code: Some("invalid_tool_call".into()),
                            request_id: None,
                            message: error,
                        },
                    )
                    .await;
            }
        };
        let auto_wait_deadline_ms = (!invocations.is_empty()).then(|| {
            completed_at_ms.saturating_add(
                i64::try_from(self.dependencies.options.auto_wait.as_millis()).unwrap_or(i64::MAX),
            )
        });
        let usage = outcome.usage.map(|usage| Usage {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cached_input_tokens: usage.cached_input_tokens,
            output_reasoning_tokens: usage.output_reasoning_tokens,
            output_text_tokens: usage.output_text_tokens,
        });
        self.append(vec![SessionEvent::StepCompleted {
            step_id,
            assistant_text: outcome.text,
            provider_calls: outcome.tool_calls,
            invocations: invocations.clone(),
            auto_wait_deadline_ms,
            usage,
            provider_context: outcome.provider_context,
            completed_at_ms,
        }])
        .await?;

        let mut executable = Vec::new();
        for invocation in invocations {
            if invocation.tool == TOOL_CANCEL_NAME {
                let target = invocation
                    .arguments
                    .get("invocation_id")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                self.cancel_tool(&invocation.invocation_id, &target).await?;
            } else {
                executable.push(invocation);
            }
        }
        self.dependencies.executor.dispatch_batch(
            &self.state.session_id,
            &self.state.workspace,
            executable,
            self.tool_result_sender.clone(),
        );
        Ok(())
    }

    async fn fail_step(&mut self, step_id: String, error: ModelError) -> Result<(), RunnerFailure> {
        self.fail_step_with_record(step_id, provider_error_record(error))
            .await
    }

    async fn fail_step_with_record(
        &mut self,
        step_id: String,
        error: ProviderErrorRecord,
    ) -> Result<(), RunnerFailure> {
        self.append(vec![SessionEvent::StepFailed {
            step_id,
            error,
            failed_at_ms: self.dependencies.clock.now_ms(),
        }])
        .await
        .map(|_| ())
    }

    async fn cancel_tool(
        &mut self,
        control_invocation_id: &str,
        target_invocation_id: &str,
    ) -> Result<(), RunnerFailure> {
        self.append(vec![SessionEvent::ToolCancelRequested {
            request_id: self.dependencies.ids.next(),
            invocation_id: target_invocation_id.to_owned(),
            requested_at_ms: self.dependencies.clock.now_ms(),
        }])
        .await?;
        let signalled = self
            .dependencies
            .executor
            .cancel(&self.state.session_id, target_invocation_id);
        let result = ToolResultData {
            result_id: self.dependencies.ids.next(),
            invocation_id: control_invocation_id.to_owned(),
            tool: TOOL_CANCEL_NAME.into(),
            outcome: ToolOutcome::Succeeded,
            message: if signalled {
                "Cancellation was signalled. The target's final ToolResult remains authoritative."
                    .into()
            } else {
                "The cancellation request was recorded, but no live target executor was found. The target's final ToolResult remains authoritative."
                    .into()
            },
            data: serde_json::json!({
                "target_invocation_id": target_invocation_id,
                "signalled": signalled,
            }),
            result_schema_version: 1,
            knowledge: None,
            finished_at_ms: self.dependencies.clock.now_ms(),
        };
        self.append(vec![SessionEvent::ToolResult { result }])
            .await
            .map(|_| ())
    }

    async fn apply_handoff(
        &mut self,
        document: Option<String>,
        failure: Option<String>,
    ) -> Result<(), RunnerFailure> {
        let previous_generation = self.state.generation.number;
        let generation = previous_generation.saturating_add(1);
        let handoff_id = self.dependencies.ids.next();
        let carried_tools = self
            .state
            .pending_tools
            .values()
            .filter(|pending| pending.result.is_none())
            .map(|pending| pending.invocation.clone())
            .collect::<Vec<_>>();
        let mut events = Vec::with_capacity(usize::from(failure.is_some()) + 1);
        if let Some(message) = failure {
            events.push(SessionEvent::HandoffFailed {
                handoff_id: handoff_id.clone(),
                generation,
                message,
                failed_at_ms: self.dependencies.clock.now_ms(),
            });
        }
        events.push(SessionEvent::HandoffApplied {
            handoff_id,
            generation,
            document,
            tools: self.dependencies.tools.initial_catalog(),
            carried_tools,
            applied_at_ms: self.dependencies.clock.now_ms(),
        });
        self.append(events).await?;

        let store = self.dependencies.store.clone();
        let session_id = self.state.session_id.clone();
        let state = snapshot_value(&self.state)
            .map_err(|error| invariant_failure("state.snapshot.serialize", error))?;
        let snapshot = tokio::task::spawn_blocking(move || {
            store.append_snapshot(&session_id, STATE_SCHEMA_VERSION, state)
        })
        .await
        .map_err(|error| infrastructure_failure("store.snapshot.task", error))?
        .map_err(|error| store_failure("store.snapshot", error))?;
        self.dependencies.observer.persisted(
            &self.state.session_id,
            std::slice::from_ref(&snapshot.envelope),
        );
        if snapshot.sealed_segment.is_some() {
            self.dependencies.compressor.wake();
        }
        self.dependencies
            .model
            .release(ModelReleaseSuggestion::Generation {
                session_id: &self.state.session_id,
                generation: previous_generation,
            });
        Ok(())
    }

    async fn persist_tool_result(&mut self, completed: CompletedTool) -> Result<(), RunnerFailure> {
        if completed.session_id != self.state.session_id {
            return Err(invariant_failure(
                "tool.result.route",
                format!(
                    "tool result for session {} reached runner {}",
                    completed.session_id, self.state.session_id
                ),
            ));
        }
        let execution = bound_tool_execution(completed.execution, &self.dependencies.options);
        let result = ToolResultData {
            result_id: self.dependencies.ids.next(),
            invocation_id: completed.invocation.invocation_id,
            tool: completed.invocation.tool,
            outcome: execution.outcome,
            message: execution.message,
            data: execution.data,
            result_schema_version: execution.result_schema_version,
            knowledge: execution.knowledge,
            finished_at_ms: self.dependencies.clock.now_ms(),
        };
        self.append(vec![SessionEvent::ToolResult { result }])
            .await
            .map(|_| ())
    }

    async fn handle_command(
        &mut self,
        command: RunnerCommand,
    ) -> Result<CommandEffect, RunnerFailure> {
        match command {
            RunnerCommand::Create {
                selection,
                system_prompt,
                workspace,
                response,
                capacity,
            } => {
                let _capacity = capacity;
                let result = if self.state.is_created() {
                    Err(RunnerRequestError {
                        message: "session already exists".into(),
                    })
                } else {
                    self.append(vec![SessionEvent::SessionCreated {
                        session_id: self.state.session_id.clone(),
                        created_at_ms: self.dependencies.clock.now_ms(),
                        selection,
                        system_prompt,
                        workspace,
                        tools: self.dependencies.tools.initial_catalog(),
                    }])
                    .await
                    .map(|_| ())
                    .map_err(request_error)
                };
                let failed = result.is_err();
                let message = result.as_ref().err().map(|error| error.message.clone());
                let _ = response.send(result);
                if failed {
                    return Err(invariant_failure(
                        "session.create",
                        message.unwrap_or_else(|| "session creation failed".into()),
                    ));
                }
                Ok(CommandEffect::Continue)
            }
            RunnerCommand::Input {
                content,
                response,
                capacity,
            } => {
                let _capacity = capacity;
                let result = self
                    .append(vec![SessionEvent::InputAppended {
                        input: Input {
                            input_id: self.dependencies.ids.next(),
                            content,
                            received_at_ms: self.dependencies.clock.now_ms(),
                        },
                    }])
                    .await;
                respond(response, &result);
                result?;
                Ok(CommandEffect::Continue)
            }
            RunnerCommand::SetSelection {
                selection,
                response,
                capacity,
            } => {
                let _capacity = capacity;
                let result = self
                    .append(vec![SessionEvent::SelectionChanged { selection }])
                    .await;
                respond(response, &result);
                result?;
                Ok(CommandEffect::Continue)
            }
            RunnerCommand::CancelTurn { response, capacity } => {
                let _capacity = capacity;
                let invocation_ids = self
                    .state
                    .active_turn
                    .as_ref()
                    .map(|turn| {
                        self.state
                            .pending_tools
                            .values()
                            .filter(|pending| {
                                pending.result.is_none()
                                    && pending.invocation.turn_id == turn.turn_id
                            })
                            .map(|pending| pending.invocation.invocation_id.clone())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let result = if let Some(turn) = &self.state.active_turn {
                    self.append(vec![SessionEvent::TurnCancelRequested {
                        turn_id: turn.turn_id.clone(),
                        requested_at_ms: self.dependencies.clock.now_ms(),
                    }])
                    .await
                } else {
                    Ok(Vec::new())
                };
                respond(response, &result);
                result?;
                for invocation_id in invocation_ids {
                    self.dependencies
                        .executor
                        .cancel(&self.state.session_id, &invocation_id);
                }
                Ok(CommandEffect::CancelTurn)
            }
            RunnerCommand::RecordFault {
                failure,
                consecutive_count,
                circuit_open,
            } => {
                self.append(vec![SessionEvent::RuntimeFault {
                    failure,
                    consecutive_count,
                    occurred_at_ms: self.dependencies.clock.now_ms(),
                }])
                .await?;
                Ok(if circuit_open {
                    CommandEffect::CircuitOpen
                } else {
                    CommandEffect::Continue
                })
            }
            RunnerCommand::Deadline(_deadline) => Ok(CommandEffect::Continue),
            RunnerCommand::Inspect(response) => {
                let _ = response.send(self.state.clone());
                Ok(CommandEffect::Continue)
            }
            RunnerCommand::Stop(response) => {
                self.dependencies
                    .executor
                    .cancel_session_and_wait(&self.state.session_id)
                    .await;
                self.dependencies
                    .deadlines
                    .cancel_session(self.state.session_id.clone())
                    .await;
                self.dependencies
                    .model
                    .release(ModelReleaseSuggestion::Session(&self.state.session_id));
                let _ = response.send(Ok(()));
                Ok(CommandEffect::Stop)
            }
        }
    }

    async fn append(
        &mut self,
        events: Vec<SessionEvent>,
    ) -> Result<Vec<EventEnvelope>, RunnerFailure> {
        if events.is_empty() {
            return Ok(Vec::new());
        }
        let store = self.dependencies.store.clone();
        let session_id = self.state.session_id.clone();
        let persisted =
            tokio::task::spawn_blocking(move || store.append_batch(&session_id, &events))
                .await
                .map_err(|error| infrastructure_failure("store.append.task", error))?
                .map_err(|error| store_failure("store.append", error))?;
        self.dependencies
            .observer
            .persisted(&self.state.session_id, &persisted);
        for envelope in &persisted {
            self.state
                .apply(&envelope.event, &self.dependencies.tools)
                .map_err(|error| invariant_failure("state.fold", error))?;
        }
        Ok(persisted)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CommandEffect {
    Continue,
    CancelTurn,
    CircuitOpen,
    Stop,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StepEffect {
    Continue,
    CircuitOpen,
    Stop,
}

struct StepStreamObserver {
    observer: Arc<dyn RunnerObserver>,
}

impl std::fmt::Debug for StepStreamObserver {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("StepStreamObserver")
    }
}

impl ModelStreamObserver for StepStreamObserver {
    fn text_delta(&self, session_id: &str, generation: u64, step_id: &str, text: &str) {
        self.observer
            .text_delta(session_id, generation, step_id, text);
    }
}

fn parse_invocations(
    state: &SessionState,
    outcome: &ModelOutcome,
    started_at_ms: i64,
    ids: &dyn IdGenerator,
) -> Result<Vec<ToolInvocation>, String> {
    let turn_id = state
        .active_turn
        .as_ref()
        .map(|turn| turn.turn_id.clone())
        .ok_or_else(|| "provider returned tool calls without an active turn".to_owned())?;
    let mut provider_ids = std::collections::BTreeSet::new();
    let mut invocations = Vec::with_capacity(outcome.tool_calls.len());
    for call in &outcome.tool_calls {
        if call.tool_call_id.is_empty() || !provider_ids.insert(call.tool_call_id.clone()) {
            return Err("provider returned an empty or duplicate tool call ID".into());
        }
        if call.tool_name != super::tools::PROVIDER_CALL_NAME {
            return Err(format!(
                "provider called {:?}; the only registered provider tool is {:?}",
                call.tool_name,
                super::tools::PROVIDER_CALL_NAME
            ));
        }
        let dynamic =
            DynamicCall::from_value(call.arguments.clone()).map_err(|error| error.to_string())?;
        invocations.push(ToolInvocation {
            invocation_id: ids.next(),
            provider_call_id: call.tool_call_id.clone(),
            turn_id: turn_id.clone(),
            started_at_ms,
            tool_version: state.known_tools.get(&dynamic.tool).cloned(),
            tool: dynamic.tool,
            arguments: dynamic.arguments,
        });
    }
    Ok(invocations)
}

fn provider_error_record(error: ModelError) -> ProviderErrorRecord {
    match error {
        ModelError::Unavailable => ProviderErrorRecord {
            stage: "model_gateway".into(),
            retryable: true,
            status_code: None,
            provider_code: Some("unavailable".into()),
            request_id: None,
            message: "Model gateway is temporarily unavailable.".into(),
        },
        ModelError::InvalidSelection => ProviderErrorRecord {
            stage: "selection".into(),
            retryable: false,
            status_code: None,
            provider_code: Some("invalid_selection".into()),
            request_id: None,
            message: "The provider selection is invalid.".into(),
        },
        ModelError::ProfileUnavailable => ProviderErrorRecord {
            stage: "profile".into(),
            retryable: false,
            status_code: None,
            provider_code: Some("profile_unavailable".into()),
            request_id: None,
            message: "The selected provider profile is unavailable or not authenticated.".into(),
        },
        ModelError::InvalidToolArguments => ProviderErrorRecord {
            stage: "provider.output.tool_call".into(),
            retryable: true,
            status_code: None,
            provider_code: Some("invalid_tool_call".into()),
            request_id: None,
            message: "The provider returned invalid tool arguments.".into(),
        },
        ModelError::ProviderFailed(failure) => ProviderErrorRecord {
            stage: failure.stage.into(),
            retryable: failure.retryable,
            status_code: failure.status_code,
            provider_code: failure.provider_code,
            request_id: failure.request_id,
            message: failure.message,
        },
    }
}

fn bound_tool_execution(mut execution: ToolExecution, options: &RunnerOptions) -> ToolExecution {
    execution.message = truncate_utf8(
        execution.message,
        options.max_tool_result_message_bytes.max(1),
    );
    let data_size = serde_json::to_vec(&execution.data)
        .map(|bytes| bytes.len())
        .unwrap_or(usize::MAX);
    if data_size > options.max_tool_result_json_bytes {
        execution.outcome = ToolOutcome::Failed;
        execution.message = format!(
            "Tool returned {data_size} bytes of JSON, exceeding the {} byte durable result limit. Side effects may already have occurred; the tool must place large display content in a file and return its path.",
            options.max_tool_result_json_bytes
        );
        execution.data = serde_json::json!({
            "result_too_large": true,
            "json_bytes": data_size,
            "limit_bytes": options.max_tool_result_json_bytes,
        });
        execution.result_schema_version = 1;
    }
    execution
}

fn truncate_utf8(mut text: String, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text;
    }
    let mut end = max_bytes;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text.push_str("\n[tool result message truncated]");
    text
}

fn estimate_input_tokens(state: &SessionState) -> Option<u64> {
    let anchor = state.anchor()?;
    let additions = state
        .generation
        .entries
        .get(anchor.entries..)
        .and_then(|entries| serde_json::to_vec(entries).ok())?;
    let estimated_addition = u64::try_from(additions.len())
        .unwrap_or(u64::MAX)
        .saturating_add(3)
        / 4;
    Some(anchor.input_tokens.saturating_add(estimated_addition))
}

fn duration_ms(duration: Duration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}

fn respond(response: Response, result: &Result<Vec<EventEnvelope>, RunnerFailure>) {
    let _ = response.send(
        result
            .as_ref()
            .map(|_| ())
            .map_err(|error| RunnerRequestError {
                message: error.message.clone(),
            }),
    );
}

fn request_error(error: RunnerFailure) -> RunnerRequestError {
    RunnerRequestError {
        message: error.message,
    }
}

fn store_failure(stage: &str, error: StoreError) -> RunnerFailure {
    RunnerFailure {
        failure_id: "store".into(),
        stage: stage.into(),
        message: error.to_string(),
    }
}

fn infrastructure_failure(stage: &str, error: impl std::fmt::Display) -> RunnerFailure {
    RunnerFailure {
        failure_id: "infrastructure".into(),
        stage: stage.into(),
        message: error.to_string(),
    }
}

fn invariant_failure(stage: &str, error: impl std::fmt::Display) -> RunnerFailure {
    RunnerFailure {
        failure_id: "runtime_invariant".into(),
        stage: stage.into(),
        message: error.to_string(),
    }
}
