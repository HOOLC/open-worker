use std::{
    collections::HashMap,
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use futures_util::{stream::FuturesUnordered, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{broadcast, watch, Mutex as AsyncMutex};

mod append;
mod commands;
mod context;
mod model;
pub mod ports;

use model::UnboundModelRequest;
pub use model::{ModelOutcome, ModelRequest, ModelTokenUsage};
pub use ports::{
    AppendResult, Clock, EventStore, ModelExecutor, ModelLimits, ModelPort, ProfileExecution,
    ProfileResolveError, ProfileResolver, RehydrateError, SessionAppendResult, SessionCreate,
    SessionCreateResult, SessionListCursor, SessionListItem, SessionListPage, SessionRef,
    StoreError, StorePort, StorePortError, TimerArm, TimerKey, TimerPort, TimerPortError,
    ToolConcurrency, ToolExecutor, ToolPort, ToolResourceAccess, VerifiedSessionState,
    MAX_SESSION_LIST_LIMIT,
};
mod stream;
mod transition;

use stream::{BroadcastModelStreamObserver, SilentModelStreamObserver};
pub use stream::{
    ModelStreamObserver, RuntimeStreamEvent, RuntimeStreamFence, RuntimeStreamMessage,
    RuntimeStreamPublisher, RuntimeStreamSubscription, TransientModelEvent,
};
use transition::*;

use crate::session::tools::{
    context_handoff_document, execute_runtime_read_tool, provider_runtime_tool_definitions,
    runtime_tool_definitions,
};
use append::*;
use context::{
    build_context_handoff_plan, context_handoff_source, estimated_model_input_tokens_from_metrics,
    model_context_generation, model_context_metrics, model_input_budget,
    model_selection_fingerprint, ContextHandoffDocumentDraft, ContextHandoffPlanDraft,
    ProviderContextCache,
};

use crate::session::state::{
    ActivationOutcome, ActiveWait, ContextHandoffDocument, ContextHandoffState, EventDraft,
    EventRecord, ModelAttemptError, ModelAttemptErrorClass, ModelAttemptFailure,
    ModelRequestPurpose, ModelRetrySchedule, ModelUsageAnchor, ProviderContext,
    ProviderInputDiagnostics, ProviderMessage, SessionEvent, SessionSelection, SessionState,
    ToolCall, TranscriptMessage, TranscriptRole, WaitSource, WAIT_MAX_SECONDS, WAIT_MIN_SECONDS,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderFailure {
    pub stage: &'static str,
    pub retryable: bool,
    pub status_code: Option<u16>,
    pub provider_code: Option<String>,
    pub request_id: Option<String>,
    pub message: String,
    pub provider_input: Option<ProviderInputDiagnostics>,
}

impl ProviderFailure {
    pub fn new(stage: &'static str, retryable: bool, message: impl Into<String>) -> Self {
        Self {
            stage,
            retryable,
            status_code: None,
            provider_code: None,
            request_id: None,
            message: message.into(),
            provider_input: None,
        }
    }
}

#[derive(Debug)]
pub enum ModelError {
    Unavailable,
    InvalidSelection,
    ProfileUnavailable,
    ProviderFailed(ProviderFailure),
    InvalidToolArguments,
}

pub const WAIT_FOR_TOOL_NAME: &str = "wait_for";
pub const END_TOOL_NAME: &str = "end";
pub const CONTEXT_HANDOFF_TOOL_NAME: &str = "context_handoff";
pub const READ_CONTEXT_HANDOFF_TOOL_NAME: &str = "read_context_handoff";
pub const READ_SESSION_HISTORY_TOOL_NAME: &str = "read_session_history";
pub const TOOL_INTERRUPTED_MESSAGE: &str = "Agent runtime was interrupted before a durable result was recorded. This tool call may have completed, partially completed, or not started. Inspect the current state before deciding whether or how to recover.";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Clone, Debug)]
pub struct ToolInvocation {
    pub session_id: String,
    pub workspace: std::path::PathBuf,
    pub tool_call_id: String,
    pub tool_name: String,
    pub input: Value,
    pub environment: std::collections::BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
pub struct ToolExecutionResult {
    pub content: String,
    pub is_error: bool,
}

impl ToolExecutionResult {
    pub fn success(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: false,
        }
    }

    pub fn error(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: true,
        }
    }
}

#[derive(Debug)]
pub enum ToolError {
    InvalidSelection,
    InvalidInvocation,
    Unavailable,
}

/// Runtime model and context budgets.
#[derive(Clone, Debug)]
pub struct RuntimeOptions {
    pub model_step_max_attempts: u32,
    pub model_retry_base: Duration,
    pub model_retry_max: Duration,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeCommandError {
    NotFound,
    Conflict,
    Invalid(&'static str),
    Backend,
    ProfileUnavailable,
}

impl RuntimeOptions {
    pub fn defaults() -> Self {
        Self {
            model_step_max_attempts: 3,
            model_retry_base: Duration::from_millis(500),
            model_retry_max: Duration::from_secs(5),
        }
    }

    fn bounded(mut self) -> Self {
        self.model_step_max_attempts = self.model_step_max_attempts.clamp(1, 32);
        self.model_retry_max = self.model_retry_max.min(Duration::from_secs(3_600));
        self.model_retry_base = self.model_retry_base.min(self.model_retry_max);
        self
    }
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct SessionKey {
    session_id: String,
}

pub struct Runtime {
    store: Arc<dyn EventStore>,
    model: Arc<dyn ModelExecutor>,
    tools: Arc<dyn ToolExecutor>,
    clock: Arc<dyn Clock>,
    timer: Arc<dyn TimerPort>,
    profiles: Arc<dyn ProfileResolver>,
    definition: AgentDefinition,
    stream_publisher: Arc<RuntimeStreamPublisher>,
    stream_observer: Arc<dyn ModelStreamObserver>,
    options: RuntimeOptions,
    session_locks: Mutex<HashMap<SessionKey, Arc<AsyncMutex<()>>>>,
    session_wakes: Mutex<HashMap<SessionKey, SessionWakeState>>,
    cancellation_signals: Mutex<HashMap<SessionKey, watch::Sender<u64>>>,
    shutting_down: AtomicBool,
}

struct SessionWakeState {
    requested: bool,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum ScheduledToolStatus {
    Pending,
    Running,
    Executed,
    Committed,
}

struct ScheduledToolCall {
    call: ToolCall,
    concurrency: ToolConcurrency,
    status: ScheduledToolStatus,
}

struct CompletedToolCall {
    index: usize,
    call: ToolCall,
    result: ToolExecutionResult,
    control: ToolResultControl,
}

type ToolCallFuture = Pin<Box<dyn Future<Output = CompletedToolCall> + Send>>;

#[derive(Clone, Debug)]
pub struct AgentDefinition {
    pub tools: Vec<String>,
    pub tool_environment: std::collections::BTreeMap<String, String>,
}

impl Runtime {
    pub fn new_with_options(
        store: Arc<dyn EventStore>,
        model: Arc<dyn ModelExecutor>,
        tools: Arc<dyn ToolExecutor>,
        options: RuntimeOptions,
        clock: Arc<dyn Clock>,
        timer: Arc<dyn TimerPort>,
        profiles: Arc<dyn ProfileResolver>,
        definition: AgentDefinition,
    ) -> Arc<Self> {
        let stream_publisher = RuntimeStreamPublisher::new(1_024);
        let stream_observer = Arc::new(BroadcastModelStreamObserver {
            publisher: stream_publisher.clone(),
        });
        Arc::new(Self {
            store,
            model,
            tools,
            clock,
            timer,
            profiles,
            definition,
            stream_publisher,
            stream_observer,
            options: options.bounded(),
            session_locks: Mutex::new(HashMap::new()),
            session_wakes: Mutex::new(HashMap::new()),
            cancellation_signals: Mutex::new(HashMap::new()),
            shutting_down: AtomicBool::new(false),
        })
    }

    /// Stops new activations; in-flight work may finish or be left for startup recovery.
    pub async fn shutdown(&self) {
        self.shutting_down.store(true, Ordering::SeqCst);
    }

    fn is_shutting_down(&self) -> bool {
        self.shutting_down.load(Ordering::SeqCst)
    }

    pub fn stream_publisher(&self) -> Arc<RuntimeStreamPublisher> {
        self.stream_publisher.clone()
    }

    pub fn now_ms(&self) -> i64 {
        self.clock.now_ms()
    }

    pub async fn queue_startup_recovery(self: &Arc<Self>) -> Result<(), &'static str> {
        self.arm_outstanding_wait_timers().await?;
        self.recover_active_activations().await?;
        self.wake_runnable_sessions().await?;
        Ok(())
    }

    /// Admit a due wait as `WaitExpired` when the durable wait is still
    /// current. Stale or replaced fires are ignored.
    pub async fn expire_wait(self: &Arc<Self>, arm: TimerArm) {
        // Shutdown is process death: do not invent WaitExpired; the next start re-arms.
        if self.is_shutting_down() {
            return;
        }
        match append_expired_timer(
            self.store.clone(),
            arm.session_id.clone(),
            arm.wait_id,
            self.clock.now_ms(),
        )
        .await
        {
            Ok(Some((append, state))) => {
                self.observe_append(&append, &state).await;
                if !append.replayed {
                    self.wake(arm.session_id);
                }
            }
            Ok(None) => {}
            Err(error) => tracing::warn!(
                error,
                session_id = arm.session_id,
                "wait timer expiry failed"
            ),
        }
    }

    fn schedule_timer(&self, session_id: String, timer: crate::session::state::WaitTimerIntent) {
        // Shutdown drops sleeps without expiry; startup recovery re-arms durable waits.
        if self.is_shutting_down() {
            return;
        }
        if let Err(error) = self.timer.arm(TimerArm {
            session_id: session_id.clone(),
            wait_id: timer.wait_id,
            deadline_ms: timer.deadline_ms,
        }) {
            tracing::warn!(error = ?error, session_id, "wait timer arm failed");
        }
    }

    async fn arm_outstanding_wait_timers(self: &Arc<Self>) -> Result<(), &'static str> {
        let store = self.store.clone();
        let timers = tokio::task::spawn_blocking(move || {
            store
                .list_outstanding_wait_timers()
                .map_err(|_| "startup_wait_list")
        })
        .await
        .map_err(|_| "startup_wait_list_join")??;
        for timer in timers {
            self.schedule_timer(
                timer.session_id,
                crate::session::state::WaitTimerIntent {
                    wait_id: timer.wait_id,
                    deadline_ms: timer.deadline_ms,
                },
            );
        }
        Ok(())
    }

    async fn recover_active_activations(self: &Arc<Self>) -> Result<(), &'static str> {
        let store = self.store.clone();
        let sessions = tokio::task::spawn_blocking(move || {
            store
                .list_active_activations()
                .map_err(|_| "startup_active_list")
        })
        .await
        .map_err(|_| "startup_active_list_join")??;
        for session in sessions {
            self.recover_startup_session(session.session_id).await?;
        }
        Ok(())
    }

    async fn wake_runnable_sessions(self: &Arc<Self>) -> Result<(), &'static str> {
        let store = self.store.clone();
        let sessions = tokio::task::spawn_blocking(move || {
            store
                .list_runnable_sessions()
                .map_err(|_| "startup_runnable_list")
        })
        .await
        .map_err(|_| "startup_runnable_list_join")??;
        for session in sessions {
            self.wake(session.session_id);
        }
        Ok(())
    }

    /// Reconcile process-bound work before exposing the readiness barrier.
    /// Every call without a durable Tool message has an unknown outcome. Record
    /// all of them atomically without invoking any tool, then let the same
    /// activation continue through its ordinary model boundary.
    async fn recover_startup_session(
        self: &Arc<Self>,
        session_id: String,
    ) -> Result<VerifiedSessionState, &'static str> {
        let mut state = rehydrate_verified(self.store.clone(), session_id.clone()).await?;
        if state.active_activation.is_some() {
            state = self
                .interrupt_unpaired_tool_calls(session_id.clone(), state)
                .await?;
            state = self.recover_model_round(session_id, state).await?;
        }
        Ok(state)
    }

    pub async fn observe_append(self: &Arc<Self>, append: &AppendResult, state: &SessionState) {
        if append.replayed {
            return;
        }
        for event in &append.events {
            self.stream_publisher
                .publish(RuntimeStreamEvent::Durable(Box::new(event.clone())));
        }
        if let Some(timer) = append.events.iter().rev().find_map(|event| {
            if let SessionEvent::WaitTimerScheduled { timer } = &event.event {
                Some(timer.clone())
            } else {
                None
            }
        }) {
            self.schedule_timer(state.session_id.clone(), timer);
        }
    }

    pub fn wake(self: &Arc<Self>, session_id: String) {
        if self.is_shutting_down() {
            return;
        }
        let key = SessionKey {
            session_id: session_id.clone(),
        };
        let should_spawn = match self.session_wakes.lock() {
            Ok(mut wakes) => {
                if let Some(wake) = wakes.get_mut(&key) {
                    wake.requested = true;
                    false
                } else {
                    wakes.insert(key.clone(), SessionWakeState { requested: true });
                    true
                }
            }
            Err(_) => false,
        };
        if should_spawn {
            self.spawn_session_runner(session_id, key);
        }
    }

    pub async fn cancel_session(
        self: &Arc<Self>,
        session_id: String,
        reason: String,
    ) -> Result<bool, RuntimeCommandError> {
        let state = rehydrate_verified(self.store.clone(), session_id.clone())
            .await
            .map_err(|_| RuntimeCommandError::NotFound)?;
        if state.active_activation.is_none() {
            return Ok(false);
        }
        let key = SessionKey {
            session_id: session_id.clone(),
        };
        let signal = self.cancellation_signal(&key)?;
        signal.send_modify(|generation| *generation = generation.wrapping_add(1));
        let lock = self.session_lock(&key)?;
        let _guard = lock.lock().await;
        self.interrupt_activation(session_id, reason)
            .await
            .map_err(|_| RuntimeCommandError::Backend)?;
        Ok(true)
    }

    fn spawn_session_runner(self: &Arc<Self>, session_id: String, key: SessionKey) {
        let runtime = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                if runtime.is_shutting_down() {
                    if let Ok(mut wakes) = runtime.session_wakes.lock() {
                        wakes.remove(&key);
                    }
                    return;
                }
                if let Ok(mut wakes) = runtime.session_wakes.lock() {
                    if let Some(wake) = wakes.get_mut(&key) {
                        wake.requested = false;
                    }
                }

                let result = match (
                    runtime.session_lock(&key),
                    runtime.cancellation_signal(&key),
                ) {
                    (Ok(lock), Ok(signal)) => {
                        let mut cancellation = signal.subscribe();
                        cancellation.borrow_and_update();
                        let _guard = lock.lock().await;
                        if runtime.is_shutting_down() {
                            Ok(())
                        } else {
                            tokio::select! {
                                result = runtime.activate(session_id.clone()) => result,
                                changed = cancellation.changed() => {
                                    if changed.is_ok() {
                                        runtime
                                            .interrupt_activation(
                                                session_id.clone(),
                                                "session_cancelled".to_owned(),
                                            )
                                            .await
                                    } else {
                                        Ok(false)
                                    }
                                    .map(|_| ())
                                    .map_err(str::to_owned)
                                }
                            }
                        }
                    }
                    _ => Err("session_runner_state".to_owned()),
                };
                runtime.model.release_session(&session_id);
                if let Err(error) = result {
                    tracing::warn!(session_id = %session_id, error, "session activation stopped");
                }

                let repeat = match runtime.session_wakes.lock() {
                    Ok(mut wakes) => {
                        let requested = wakes
                            .get(&key)
                            .is_some_and(|wake| wake.requested && !runtime.is_shutting_down());
                        if !requested {
                            wakes.remove(&key);
                        }
                        requested
                    }
                    Err(_) => false,
                };
                if !repeat {
                    return;
                }
            }
        });
    }

    fn session_lock(&self, key: &SessionKey) -> Result<Arc<AsyncMutex<()>>, RuntimeCommandError> {
        self.session_locks
            .lock()
            .map_err(|_| RuntimeCommandError::Backend)
            .map(|mut locks| {
                locks
                    .entry(key.clone())
                    .or_insert_with(|| Arc::new(AsyncMutex::new(())))
                    .clone()
            })
    }

    fn cancellation_signal(
        &self,
        key: &SessionKey,
    ) -> Result<watch::Sender<u64>, RuntimeCommandError> {
        self.cancellation_signals
            .lock()
            .map_err(|_| RuntimeCommandError::Backend)
            .map(|mut signals| {
                signals
                    .entry(key.clone())
                    .or_insert_with(|| watch::channel(0).0)
                    .clone()
            })
    }

    async fn interrupt_activation(
        self: &Arc<Self>,
        session_id: String,
        reason: String,
    ) -> Result<bool, &'static str> {
        let state = rehydrate_verified(self.store.clone(), session_id.clone()).await?;
        let Some(activation) = state.active_activation.as_ref() else {
            return Ok(false);
        };
        let activation_id = activation.activation_id.clone();
        let now_ms = self.clock.now_ms();
        let mut lifecycle_events = Vec::new();
        if let Some(round) = state.active_model_round.as_ref() {
            if let (Some(request), Some(attempt)) = (round.request.as_ref(), round.attempt.as_ref())
            {
                if attempt.outcome == crate::session::state::ModelAttemptOutcome::Running {
                    lifecycle_events.push(SessionEvent::ModelAttemptInterrupted {
                        activation_id: activation_id.clone(),
                        round_id: round.round_id.clone(),
                        request_id: request.request_id.clone(),
                        attempt_id: attempt.attempt_id.clone(),
                        attempt_number: attempt.attempt_number,
                        reason: reason.clone(),
                    });
                    lifecycle_events.push(SessionEvent::ModelRequestAbandoned {
                        activation_id: activation_id.clone(),
                        round_id: round.round_id.clone(),
                        request_id: request.request_id.clone(),
                        attempt_id: attempt.attempt_id.clone(),
                        reason: reason.clone(),
                        abandoned_at_ms: now_ms,
                    });
                }
            }
        }
        let interrupted_tool_call_ids = pending_tool_calls(&state)
            .into_iter()
            .map(|call| call.tool_call_id)
            .collect::<Vec<_>>();
        let wait_id = state.active_wait.as_ref().map(|wait| wait.wait_id.clone());
        let event_count = lifecycle_events.len()
            + interrupted_tool_call_ids.len()
            + usize::from(wait_id.is_some())
            + 1;
        let (append, next_state) =
            append_runtime_drafts_from_state(self.store.clone(), session_id, state, move |_| {
                Ok(EventDraft::batch(event_count, |event_ids| {
                    let mut events = lifecycle_events.clone();
                    for tool_call_id in &interrupted_tool_call_ids {
                        let message_id = event_ids[events.len()].clone();
                        events.push(SessionEvent::MessageAppended {
                            message: TranscriptMessage {
                                message_id,
                                role: TranscriptRole::Tool,
                                content: format!(
                                    "Agent activation was interrupted before a durable result was recorded ({reason}). This tool call may have completed, partially completed, or not started. Inspect the current state before deciding whether or how to recover."
                                )
                                .into(),
                                is_error: true,
                                tool_call_id: Some(tool_call_id.clone()),
                                tool_calls: Vec::new(),
                                provider_context: None,
                                source_mailbox_seq: None,
                            },
                            wake_wait: false,
                        });
                    }
                    if let Some(wait_id) = &wait_id {
                        events.push(SessionEvent::WaitCleared {
                            wait_id: wait_id.clone(),
                        });
                    }
                    events.push(SessionEvent::ActivationFinished {
                        activation_id: activation_id.clone(),
                        outcome: ActivationOutcome::Interrupted,
                        finished_at_ms: now_ms,
                    });
                    events
                }))
            })
            .await?;
        self.observe_append(&append, &next_state).await;
        Ok(true)
    }

    async fn activate(self: &Arc<Self>, session_id: String) -> Result<(), String> {
        let mut state = rehydrate_verified(self.store.clone(), session_id.clone()).await?;
        let selection = state
            .active_activation
            .as_ref()
            .map(|activation| activation.selection.clone())
            .unwrap_or_else(|| state.selection.clone());

        if state.active_activation.is_none() {
            // Wake requests may already be queued behind the per-session lock
            // when a prior activation reaches a terminal handoff failure.
            // Re-check durable runnable state after acquiring that lock so a
            // stale wake cannot start the same failed handoff again.
            if !state.is_startup_runnable() {
                return Ok(());
            }
            let Some((append, next_state)) = start_activation(
                self.store.clone(),
                self.clock.clone(),
                session_id.clone(),
                &state,
            )
            .await?
            else {
                return Ok(());
            };
            self.observe_append(&append, &next_state).await;
            state = next_state;
        }

        // A tool batch returned directly by `run_model_round` is owned and
        // executed by that call. Reaching a later activation entry with
        // unpaired calls means their owner disappeared without a durable
        // result, whether because of process restart or a stopped runner.
        state = self
            .interrupt_unpaired_tool_calls(session_id.clone(), state)
            .await?;
        state = self.recover_model_round(session_id.clone(), state).await?;
        let mut context_cache = ProviderContextCache::default();
        loop {
            if state.has_inflight_tool_effect() {
                return Err("unpaired_tool_call_after_reconciliation".to_owned());
            }
            if state.active_wait.is_some() && state.mailbox.is_empty() {
                if let Some(activation) = state.active_activation.as_ref() {
                    if let Some((append, next_state)) = finish_activation(
                        self.store.clone(),
                        self.clock.clone(),
                        session_id.clone(),
                        &state,
                        activation.activation_id.clone(),
                        ActivationOutcome::Wait,
                    )
                    .await?
                    {
                        self.observe_append(&append, &next_state).await;
                    }
                }
                return Ok(());
            }

            let (append, next_state) =
                drain_mailbox(self.store.clone(), session_id.clone(), state).await?;
            if let Some(append) = append {
                self.observe_append(&append, &next_state).await;
                state = next_state;
                continue;
            }
            state = next_state;

            if state.unresolved_input().is_some() {
                let (next_state, prepared) = self
                    .ensure_model_context(&session_id, &selection, state, &mut context_cache)
                    .await?;
                state = next_state;
                if state.active_activation.is_none() {
                    return Ok(());
                }
                if prepared.is_none() && (!state.mailbox.is_empty() || state.active_wait.is_some())
                {
                    continue;
                }
                let prepared = prepared.ok_or("model_context_missing")?;
                let round = self
                    .run_model_round(&session_id, &selection, &state, prepared)
                    .await;
                let (appends, next_state) = match round {
                    Ok(round) => round,
                    Err(error) => {
                        return Err(error);
                    }
                };
                for (append, state_after_append) in appends {
                    self.observe_append(&append, &state_after_append).await;
                }
                state = next_state;
                if state.active_activation.is_none() {
                    return Ok(());
                }
                continue;
            }

            if state.model_followup_identity().is_some() {
                let (next_state, prepared) = self
                    .ensure_model_context(&session_id, &selection, state, &mut context_cache)
                    .await?;
                state = next_state;
                if state.active_activation.is_none() {
                    return Ok(());
                }
                if prepared.is_none() && (!state.mailbox.is_empty() || state.active_wait.is_some())
                {
                    continue;
                }
                let prepared = prepared.ok_or("model_context_missing")?;
                let round = self
                    .run_model_round(&session_id, &selection, &state, prepared)
                    .await;
                let (appends, next_state) = match round {
                    Ok(round) => round,
                    Err(error) => {
                        return Err(error);
                    }
                };
                for (append, state_after_append) in appends {
                    self.observe_append(&append, &state_after_append).await;
                }
                state = next_state;
                if state.active_activation.is_none() {
                    return Ok(());
                }
            } else {
                return Err("active_activation_without_next_action".to_owned());
            }
        }
    }

    async fn finish_model_failure_activation(
        self: &Arc<Self>,
        session_id: &str,
        state: VerifiedSessionState,
    ) -> Result<VerifiedSessionState, &'static str> {
        let Some(activation) = state.active_activation.as_ref() else {
            return Ok(state);
        };
        let Some((append, next_state)) = finish_activation(
            self.store.clone(),
            self.clock.clone(),
            session_id.to_owned(),
            &state,
            activation.activation_id.clone(),
            ActivationOutcome::Failed,
        )
        .await?
        else {
            return Ok(state);
        };
        self.observe_append(&append, &next_state).await;
        if !next_state.mailbox.is_empty() {
            self.wake(session_id.to_owned());
        }
        Ok(next_state)
    }

    pub(super) async fn execute_pending_tool_calls(
        self: &Arc<Self>,
        session_id: &str,
        mut state: VerifiedSessionState,
    ) -> Result<VerifiedSessionState, &'static str> {
        let calls = pending_tool_calls(&state);
        if calls.is_empty() && state.has_inflight_tool_effect() {
            return Err("inflight_tool_batch_missing");
        }
        let mut definitions = self
            .tools
            .definitions(&self.definition.tools)
            .map_err(|_| "tool_selection")?;
        definitions.extend(runtime_tool_definitions());
        let definitions = definitions
            .into_iter()
            .map(|definition| (definition.name.clone(), definition))
            .collect::<HashMap<_, _>>();
        let definitions = Arc::new(definitions);
        let mut scheduled = calls
            .into_iter()
            .map(|call| ScheduledToolCall {
                concurrency: self.tool_concurrency(&state, &call),
                call,
                status: ScheduledToolStatus::Pending,
            })
            .collect::<Vec<_>>();
        let mut running = FuturesUnordered::<ToolCallFuture>::new();
        let mut completed_results = (0..scheduled.len())
            .map(|_| None)
            .collect::<Vec<Option<CompletedToolCall>>>();
        let mut next_to_commit = 0usize;

        while next_to_commit < scheduled.len() {
            while let Some(completed_call) = completed_results[next_to_commit].take() {
                let appended = append_tool_result(
                    self.store.clone(),
                    session_id.to_owned(),
                    state,
                    completed_call.call,
                    completed_call.result,
                    completed_call.control,
                )
                .await?;
                self.observe_append(&appended.0, &appended.1).await;
                state = appended.1;
                scheduled[next_to_commit].status = ScheduledToolStatus::Committed;
                next_to_commit += 1;
                if next_to_commit == scheduled.len() {
                    break;
                }
            }
            if next_to_commit == scheduled.len() {
                break;
            }

            for index in 0..scheduled.len() {
                if scheduled[index].status != ScheduledToolStatus::Pending {
                    continue;
                }
                let blocked = scheduled[..index].iter().any(|prior| {
                    prior.status != ScheduledToolStatus::Committed
                        && tool_concurrency_conflicts(
                            &prior.concurrency,
                            &scheduled[index].concurrency,
                        )
                });
                if blocked {
                    continue;
                }
                scheduled[index].status = ScheduledToolStatus::Running;
                let runtime = Arc::clone(self);
                let tool_session_id = session_id.to_owned();
                let tool_state = state.clone();
                let tool_definitions = definitions.clone();
                let call = scheduled[index].call.clone();
                running.push(Box::pin(async move {
                    let (result, control) = runtime
                        .execute_tool_call(
                            &tool_session_id,
                            &tool_state,
                            tool_definitions.as_ref(),
                            &call,
                        )
                        .await;
                    CompletedToolCall {
                        index,
                        call,
                        result,
                        control,
                    }
                }));
            }

            let completed_call = running
                .next()
                .await
                .ok_or("tool_scheduler_without_runnable_call")?;
            let completed_index = completed_call.index;
            scheduled[completed_index].status = ScheduledToolStatus::Executed;
            completed_results[completed_index] = Some(completed_call);
        }
        Ok(state)
    }

    fn tool_concurrency(&self, state: &SessionState, call: &ToolCall) -> ToolConcurrency {
        match call.tool_name.as_str() {
            END_TOOL_NAME | WAIT_FOR_TOOL_NAME | CONTEXT_HANDOFF_TOOL_NAME => {
                ToolConcurrency::Exclusive
            }
            READ_CONTEXT_HANDOFF_TOOL_NAME | READ_SESSION_HISTORY_TOOL_NAME => {
                ToolConcurrency::Parallel
            }
            _ => self.tools.concurrency(&self.tool_invocation(state, call)),
        }
    }

    fn tool_invocation(&self, state: &SessionState, call: &ToolCall) -> ToolInvocation {
        ToolInvocation {
            session_id: state.session_id.clone(),
            workspace: std::path::PathBuf::from(&state.workspace),
            tool_call_id: call.tool_call_id.clone(),
            tool_name: call.tool_name.clone(),
            input: call.arguments.clone(),
            environment: self.definition.tool_environment.clone(),
        }
    }

    async fn execute_tool_call(
        self: &Arc<Self>,
        session_id: &str,
        state: &VerifiedSessionState,
        definitions: &HashMap<String, ToolDefinition>,
        call: &ToolCall,
    ) -> (ToolExecutionResult, ToolResultControl) {
        if call.tool_name == END_TOOL_NAME {
            return evaluate_end_tool_call(state, call, self.clock.now_ms());
        }
        if call.tool_name == WAIT_FOR_TOOL_NAME {
            return match parse_wait(call, &*self.clock) {
                Ok(wait) => (
                    ToolExecutionResult::success("wait_for accepted"),
                    ToolResultControl::Wait(wait),
                ),
                Err(_) => (
                    ToolExecutionResult::error("Invalid arguments for tool wait_for"),
                    ToolResultControl::Continue,
                ),
            };
        }
        if call.tool_name == CONTEXT_HANDOFF_TOOL_NAME {
            return match context_handoff_document(&call.arguments) {
                Ok(_) => (
                    ToolExecutionResult::error(
                        "context_handoff is only available when the runtime requests a context handoff",
                    ),
                    ToolResultControl::Continue,
                ),
                Err(_) => (
                    ToolExecutionResult::error("Invalid arguments for tool context_handoff"),
                    ToolResultControl::Continue,
                ),
            };
        }
        let Some(definition) = definitions.get(&call.tool_name) else {
            return (
                ToolExecutionResult::error(format!(
                    "Tool {} is not available in this session",
                    call.tool_name
                )),
                ToolResultControl::Continue,
            );
        };
        let schema_valid = jsonschema::validator_for(&definition.input_schema)
            .map(|validator| validator.is_valid(&call.arguments))
            .unwrap_or(false);
        if !schema_valid {
            return (
                ToolExecutionResult::error(format!(
                    "Invalid arguments for tool {}",
                    call.tool_name
                )),
                ToolResultControl::Continue,
            );
        }
        if matches!(
            call.tool_name.as_str(),
            READ_CONTEXT_HANDOFF_TOOL_NAME | READ_SESSION_HISTORY_TOOL_NAME
        ) {
            let store = self.store.clone();
            let read_session_id = session_id.to_owned();
            let read_state = state.clone();
            let tool_name = call.tool_name.clone();
            let arguments = call.arguments.clone();
            let result = tokio::task::spawn_blocking(move || {
                execute_runtime_read_tool(
                    &read_state,
                    store.as_ref(),
                    &read_session_id,
                    &tool_name,
                    &arguments,
                )
            })
            .await
            .unwrap_or(Err(ToolError::Unavailable));
            return (
                result.unwrap_or_else(tool_error_result),
                ToolResultControl::Continue,
            );
        }
        (
            self.tools
                .execute(self.tool_invocation(state, call))
                .await
                .unwrap_or_else(tool_error_result),
            ToolResultControl::Continue,
        )
    }
}

fn tool_concurrency_conflicts(left: &ToolConcurrency, right: &ToolConcurrency) -> bool {
    match (left, right) {
        (ToolConcurrency::Exclusive, _) | (_, ToolConcurrency::Exclusive) => true,
        (
            ToolConcurrency::Resource {
                key: left_key,
                access: left_access,
            },
            ToolConcurrency::Resource {
                key: right_key,
                access: right_access,
            },
        ) => {
            left_key == right_key
                && (*left_access == ToolResourceAccess::Write
                    || *right_access == ToolResourceAccess::Write)
        }
        _ => false,
    }
}

/// Return the first mailbox input that follows an exhausted model attempt which
/// never committed an assistant message.  The empty assistant is provider
/// context only: it preserves the failed round's provider-context boundary without
/// adding a public transcript event.  A normal multi-user first round has no
/// exhaustion fact (or has an assistant between the users), so it is not
/// modified.
fn failed_round_placeholder_target(state: &SessionState) -> Option<String> {
    state.last_model_attempts_exhausted.as_ref()?;
    let failure = state.last_model_attempt_failure.as_ref()?;
    let failure_index = state.transcript.iter().position(|message| {
        message.role.is_mailbox_input() && message.message_id == failure.trigger_message_id
    })?;
    if !state
        .transcript
        .last()
        .is_some_and(|message| message.role.is_mailbox_input())
    {
        return None;
    }
    let mut assistant_seen = false;
    for message in state.transcript.iter().skip(failure_index + 1) {
        match message.role {
            TranscriptRole::Assistant => assistant_seen = true,
            TranscriptRole::User => {
                return (!assistant_seen).then(|| message.message_id.clone());
            }
            TranscriptRole::System | TranscriptRole::Tool => {}
        }
    }
    None
}

fn pending_tool_calls(state: &SessionState) -> Vec<ToolCall> {
    state
        .transcript
        .iter()
        .rev()
        .find(|message| {
            message.role == TranscriptRole::Assistant
                && message
                    .tool_calls
                    .iter()
                    .any(|call| state.inflight_tool_call_ids.contains(&call.tool_call_id))
        })
        .map(|message| {
            message
                .tool_calls
                .iter()
                .filter(|call| state.inflight_tool_call_ids.contains(&call.tool_call_id))
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

fn is_only_tool_call_in_batch(state: &SessionState, call: &ToolCall) -> bool {
    state.transcript.iter().rev().any(|message| {
        message.role == TranscriptRole::Assistant
            && message.tool_calls.len() == 1
            && message
                .tool_calls
                .first()
                .is_some_and(|candidate| candidate.tool_call_id == call.tool_call_id)
    })
}

fn evaluate_end_tool_call(
    state: &SessionState,
    call: &ToolCall,
    finished_at_ms: i64,
) -> (ToolExecutionResult, ToolResultControl) {
    if !call
        .arguments
        .as_object()
        .is_some_and(serde_json::Map::is_empty)
    {
        return (
            ToolExecutionResult::error("Invalid arguments for tool end"),
            ToolResultControl::Continue,
        );
    }
    if !is_only_tool_call_in_batch(state, call) {
        return (
            ToolExecutionResult::error("end must be the only tool call in its model response"),
            ToolResultControl::Continue,
        );
    }
    let Some(activation) = state.active_activation.as_ref() else {
        return (
            ToolExecutionResult::error("end requires an active activation"),
            ToolResultControl::Continue,
        );
    };
    (
        ToolExecutionResult::success("end accepted"),
        ToolResultControl::FinishActivation {
            activation_id: activation.activation_id.clone(),
            finished_at_ms,
        },
    )
}

fn tool_error_result(error: ToolError) -> ToolExecutionResult {
    let message = match error {
        ToolError::InvalidSelection => "Tool is not available in this session",
        ToolError::InvalidInvocation => "Invalid tool arguments",
        ToolError::Unavailable => "Tool execution is unavailable",
    };
    ToolExecutionResult::error(message)
}

fn stable_fingerprint(kind: &str, value: &str) -> String {
    use sha2::{Digest, Sha256};

    let mut digest = Sha256::new();
    digest.update(b"zork:runtime-fingerprint:v1");
    digest.update((kind.len() as u64).to_be_bytes());
    digest.update(kind.as_bytes());
    digest.update((value.len() as u64).to_be_bytes());
    digest.update(value.as_bytes());
    format!("sha256:v1:{:x}", digest.finalize())
}

#[cfg(test)]
mod tool_scheduling_tests {
    use super::*;
    use crate::session::{
        store::JsonlEventStore,
        timer::{SleepTimer, SystemClock},
    };
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
    use tokio::sync::{Barrier, Semaphore};

    struct UnusedModel;

    impl ModelExecutor for UnusedModel {
        fn complete<'a>(
            &'a self,
            _request: &'a ModelRequest,
            _execution: ProfileExecution,
        ) -> Pin<Box<dyn Future<Output = Result<ModelOutcome, ModelError>> + Send + 'a>> {
            Box::pin(async { Err(ModelError::Unavailable) })
        }
    }

    struct ReorderedParallelTools {
        start_barrier: Arc<Barrier>,
        first_release: Arc<Semaphore>,
        started: AtomicUsize,
    }

    impl ToolExecutor for ReorderedParallelTools {
        fn definitions(&self, selected: &[String]) -> Result<Vec<ToolDefinition>, ToolError> {
            Ok(selected
                .iter()
                .map(|name| ToolDefinition {
                    name: name.clone(),
                    description: format!("test tool {name}"),
                    input_schema: serde_json::json!({
                        "type": "object",
                        "additionalProperties": false
                    }),
                })
                .collect::<Vec<_>>())
        }

        fn concurrency(&self, _invocation: &ToolInvocation) -> ToolConcurrency {
            ToolConcurrency::Parallel
        }

        fn execute<'a>(
            &'a self,
            invocation: ToolInvocation,
        ) -> Pin<Box<dyn Future<Output = Result<ToolExecutionResult, ToolError>> + Send + 'a>>
        {
            Box::pin(async move {
                self.started.fetch_add(1, AtomicOrdering::SeqCst);
                self.start_barrier.wait().await;
                if invocation.tool_name == "first" {
                    self.first_release
                        .clone()
                        .acquire_owned()
                        .await
                        .map_err(|_| ToolError::Unavailable)?
                        .forget();
                }
                Ok(ToolExecutionResult::success(invocation.tool_name))
            })
        }
    }

    #[test]
    fn provider_failure_retryability_is_not_hardcoded() {
        let failure = |retryable| {
            ModelError::ProviderFailed(ProviderFailure {
                stage: "provider.test",
                retryable,
                status_code: None,
                provider_code: None,
                request_id: None,
                message: "test failure".to_owned(),
                provider_input: None,
            })
        };

        assert!(model::model_error_is_retryable(&failure(true)));
        assert!(!model::model_error_is_retryable(&failure(false)));
    }

    #[test]
    fn concurrency_conflicts_only_for_exclusive_or_same_resource_write() {
        let parallel = ToolConcurrency::Parallel;
        let exclusive = ToolConcurrency::Exclusive;
        let read_a = ToolConcurrency::Resource {
            key: "a".to_owned(),
            access: ToolResourceAccess::Read,
        };
        let write_a = ToolConcurrency::Resource {
            key: "a".to_owned(),
            access: ToolResourceAccess::Write,
        };
        let write_b = ToolConcurrency::Resource {
            key: "b".to_owned(),
            access: ToolResourceAccess::Write,
        };

        assert!(!tool_concurrency_conflicts(&parallel, &parallel));
        assert!(tool_concurrency_conflicts(&parallel, &exclusive));
        assert!(tool_concurrency_conflicts(&exclusive, &parallel));
        assert!(!tool_concurrency_conflicts(&read_a, &read_a));
        assert!(tool_concurrency_conflicts(&read_a, &write_a));
        assert!(tool_concurrency_conflicts(&write_a, &read_a));
        assert!(tool_concurrency_conflicts(&write_a, &write_a));
        assert!(!tool_concurrency_conflicts(&write_a, &write_b));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn parallel_tools_execute_concurrently_but_commit_in_declaration_order() {
        let temporary = tempfile::tempdir().unwrap();
        let store = Arc::new(JsonlEventStore::open(temporary.path().join("sessions")).unwrap());
        let selection = SessionSelection {
            profile_id: "profile".to_owned(),
            model: "model".to_owned(),
            thinking: "off".to_owned(),
        };
        let created = store
            .create_session(&SessionCreate {
                created_at_ms: 1,
                selection,
                system_prompt: None,
                workspace: temporary.path().to_string_lossy().into_owned(),
            })
            .unwrap();
        let session_id = created.state.session_id.clone();
        let calls = [("call-first", "first"), ("call-second", "second")]
            .into_iter()
            .map(|(tool_call_id, tool_name)| ToolCall {
                tool_call_id: tool_call_id.to_owned(),
                tool_name: tool_name.to_owned(),
                arguments: serde_json::json!({}),
            })
            .collect::<Vec<_>>();
        let assistant = EventDraft::identified(|message_id| SessionEvent::MessageAppended {
            message: TranscriptMessage {
                message_id: message_id.to_owned(),
                role: TranscriptRole::Assistant,
                content: Arc::from(""),
                is_error: false,
                tool_call_id: None,
                tool_calls: calls,
                provider_context: None,
                source_mailbox_seq: None,
            },
            wake_wait: false,
        });
        let state = store
            .append(&session_id, &created.state, &[assistant])
            .unwrap()
            .state;

        let start_barrier = Arc::new(Barrier::new(3));
        let first_release = Arc::new(Semaphore::new(0));
        let tools = Arc::new(ReorderedParallelTools {
            start_barrier: start_barrier.clone(),
            first_release: first_release.clone(),
            started: AtomicUsize::new(0),
        });
        let clock: Arc<dyn Clock> = Arc::new(SystemClock);
        let (due_tx, _due_rx) = tokio::sync::mpsc::unbounded_channel();
        let timer = Arc::new(SleepTimer::new(clock.clone(), due_tx));
        let profiles = Arc::new(crate::profiles::ProfileStore::open(
            temporary.path().join("profiles"),
            true,
            false,
        ));
        let runtime = Runtime::new_with_options(
            store.clone(),
            Arc::new(UnusedModel),
            tools.clone(),
            RuntimeOptions::defaults(),
            clock,
            timer,
            profiles,
            AgentDefinition {
                tools: vec!["first".to_owned(), "second".to_owned()],
                tool_environment: Default::default(),
            },
        );

        let execution = tokio::spawn({
            let runtime = runtime.clone();
            let session_id = session_id.clone();
            async move {
                runtime
                    .execute_pending_tool_calls(&session_id, state)
                    .await
                    .unwrap()
            }
        });
        start_barrier.wait().await;
        assert_eq!(tools.started.load(AtomicOrdering::SeqCst), 2);

        let later_result_was_committed_early =
            tokio::time::timeout(Duration::from_millis(100), async {
                loop {
                    let state = store.rehydrate(&session_id).unwrap();
                    if state.transcript.iter().any(|message| {
                        message.role == TranscriptRole::Tool
                            && message.tool_call_id.as_deref() == Some("call-second")
                    }) {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await
            .is_ok();
        first_release.add_permits(1);
        let state = tokio::time::timeout(Duration::from_secs(2), execution)
            .await
            .unwrap()
            .unwrap();

        assert!(
            !later_result_was_committed_early,
            "the second result became durable while the first call was still running"
        );
        let tool_result_order = state
            .transcript
            .iter()
            .filter(|message| message.role == TranscriptRole::Tool)
            .map(|message| message.tool_call_id.as_deref().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(tool_result_order, vec!["call-first", "call-second"]);
    }
}
