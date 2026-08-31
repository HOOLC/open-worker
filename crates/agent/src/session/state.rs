//! Pure event fold and snapshot-state migration.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::events::{
    AutoWaitEndReason, DeadlineKind, Input, OutstandingItem, ProviderErrorRecord, Purpose,
    Selection, SessionEvent, ToolDelivery, ToolDeliveryMode, ToolInvocation, ToolResultData,
    TurnOutcome, Usage, END_TOOL_NAME, WAIT_TOOL_NAME,
};
use super::tools::{
    ToolChange, ToolIntroduction, ToolKnowledge, ToolRegistry, ToolState, ToolVersion,
};
use super::wire::{ProviderContext, ProviderToolCall};

pub const STATE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GenerationEntry {
    Inputs {
        inputs: Vec<Input>,
    },
    ToolChanges {
        changes: Vec<ToolChange>,
    },
    Notice {
        message: String,
    },
    Outstanding {
        items: Vec<OutstandingItem>,
    },
    ToolDelivery {
        delivery: ToolDelivery,
    },
    Assistant {
        step_id: String,
        text: String,
        provider_calls: Vec<ProviderToolCall>,
        invocations: Vec<ToolInvocation>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<Usage>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_context: Option<ProviderContext>,
    },
    CarriedTools {
        invocations: Vec<ToolInvocation>,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct GenerationState {
    pub number: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handoff_document: Option<String>,
    pub tools: Vec<ToolIntroduction>,
    pub entries: Vec<GenerationEntry>,
}

impl GenerationState {
    fn new(number: u64, handoff_document: Option<String>, tools: Vec<ToolIntroduction>) -> Self {
        Self {
            number,
            handoff_document,
            tools,
            entries: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ActiveTurn {
    pub turn_id: String,
    pub started_at_ms: i64,
    pub cancel_requested: bool,
    pub consecutive_provider_failures: u32,
    pub provider_retry_allowed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handoff: Option<HandoffProgress>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HandoffProgress {
    pub started_at_ms: i64,
    pub attempts: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StepFailureState {
    pub purpose: Purpose,
    pub error: ProviderErrorRecord,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ActiveStep {
    pub step_id: String,
    pub turn_id: String,
    pub purpose: Purpose,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection: Option<Selection>,
    pub request_entries: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub consumed_inputs: Vec<Input>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    pub started_at_ms: i64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolDeliveryState {
    Fresh,
    PendingSent,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PendingTool {
    pub invocation: ToolInvocation,
    pub delivery: ToolDeliveryState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<ToolResultData>,
    pub cancel_requested: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AutoWait {
    pub step_id: String,
    pub invocation_ids: Vec<String>,
    pub deadline_ms: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WaitDeadline {
    pub invocation_id: String,
    pub deadline_ms: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TokenAnchor {
    pub input_tokens: u64,
    pub generation: u64,
    pub entries: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FaultStreak {
    pub fingerprint: String,
    pub count: u32,
    pub completed_steps_after_fault: u32,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SessionState {
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection: Option<Selection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    pub workspace: String,

    pub generation: GenerationState,
    pub unconsumed_inputs: Vec<Input>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_turn: Option<ActiveTurn>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_step: Option<ActiveStep>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_turn_outcome: Option<TurnOutcome>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_step_failure: Option<StepFailureState>,

    pub pending_tools: BTreeMap<String, PendingTool>,
    pub known_tools: BTreeMap<String, ToolVersion>,
    pub tool_states: BTreeMap<String, ToolState>,
    pub pending_notices: Vec<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_wait: Option<AutoWait>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait_deadline: Option<WaitDeadline>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_anchor: Option<TokenAnchor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fault_streak: Option<FaultStreak>,
}

impl SessionState {
    pub fn empty(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            created_at_ms: None,
            selection: None,
            system_prompt: None,
            workspace: String::new(),
            generation: GenerationState::new(0, None, Vec::new()),
            unconsumed_inputs: Vec::new(),
            active_turn: None,
            active_step: None,
            last_turn_outcome: None,
            last_step_failure: None,
            pending_tools: BTreeMap::new(),
            known_tools: BTreeMap::new(),
            tool_states: BTreeMap::new(),
            pending_notices: Vec::new(),
            auto_wait: None,
            wait_deadline: None,
            token_anchor: None,
            fault_streak: None,
        }
    }

    pub fn is_created(&self) -> bool {
        self.created_at_ms.is_some()
    }

    pub fn apply(&mut self, event: &SessionEvent, tools: &ToolRegistry) -> Result<(), FoldError> {
        if !self.is_created() && !matches!(event, SessionEvent::SessionCreated { .. }) {
            return Err(FoldError::MissingSessionCreated);
        }

        match event {
            SessionEvent::SessionCreated {
                session_id,
                created_at_ms,
                selection,
                system_prompt,
                workspace,
                tools,
            } => self.apply_created(
                session_id,
                *created_at_ms,
                selection,
                system_prompt,
                workspace,
                tools,
            )?,
            SessionEvent::InputAppended { input } => {
                self.unconsumed_inputs.push(input.clone());
            }
            SessionEvent::SelectionChanged { selection } => {
                self.selection = Some(selection.clone());
                self.token_anchor = None;
            }
            SessionEvent::TurnStarted {
                turn_id,
                started_at_ms,
            } => {
                if self.active_turn.is_some() {
                    self.diagnose("A new turn started while another turn was still active.");
                }
                self.active_turn = Some(ActiveTurn {
                    turn_id: turn_id.clone(),
                    started_at_ms: *started_at_ms,
                    cancel_requested: false,
                    consecutive_provider_failures: 0,
                    provider_retry_allowed: true,
                    handoff: None,
                });
                self.last_turn_outcome = None;
            }
            SessionEvent::TurnCancelRequested { turn_id, .. } => {
                if let Some(turn) = self
                    .active_turn
                    .as_mut()
                    .filter(|turn| turn.turn_id == *turn_id)
                {
                    turn.cancel_requested = true;
                } else {
                    self.diagnose(format!(
                        "A cancellation referred to non-active turn {turn_id}."
                    ));
                }
            }
            SessionEvent::TurnFinished {
                turn_id, outcome, ..
            } => {
                if self
                    .active_turn
                    .as_ref()
                    .is_some_and(|turn| turn.turn_id != *turn_id)
                {
                    self.diagnose(format!(
                        "Turn {turn_id} finished while a different turn was active."
                    ));
                }
                self.active_turn = None;
                self.active_step = None;
                self.auto_wait = None;
                self.last_turn_outcome = Some(*outcome);
                self.pending_tools.retain(|_, pending| {
                    pending.result.is_none() || pending.invocation.tool != END_TOOL_NAME
                });
            }
            SessionEvent::StepStarted {
                step_id,
                turn_id,
                purpose,
                consumed_inputs,
                deliveries,
                tool_changes,
                notices,
                outstanding,
                max_output_tokens,
                started_at_ms,
            } => self.apply_step_started(
                step_id,
                turn_id,
                *purpose,
                consumed_inputs,
                deliveries,
                tool_changes,
                notices,
                outstanding,
                *max_output_tokens,
                *started_at_ms,
            ),
            SessionEvent::StepCompleted {
                step_id,
                assistant_text,
                provider_calls,
                invocations,
                auto_wait_deadline_ms,
                usage,
                provider_context,
                completed_at_ms,
            } => self.apply_step_completed(
                step_id,
                assistant_text,
                provider_calls,
                invocations,
                *auto_wait_deadline_ms,
                usage,
                provider_context,
                *completed_at_ms,
            ),
            SessionEvent::StepFailed { step_id, error, .. } => {
                let purpose = self
                    .active_step
                    .as_ref()
                    .filter(|step| step.step_id == *step_id)
                    .map_or(Purpose::Conversation, |step| step.purpose);
                if error.is_context_overflow() {
                    if let Some(step) = self
                        .active_step
                        .as_ref()
                        .filter(|step| step.step_id == *step_id)
                    {
                        let existing = self
                            .unconsumed_inputs
                            .iter()
                            .map(|input| input.input_id.clone())
                            .collect::<BTreeSet<_>>();
                        let mut restored = step
                            .consumed_inputs
                            .iter()
                            .filter(|input| !existing.contains(&input.input_id))
                            .cloned()
                            .collect::<Vec<_>>();
                        restored.append(&mut self.unconsumed_inputs);
                        self.unconsumed_inputs = restored;
                    }
                }
                self.close_step(step_id, "failed");
                if let Some(turn) = &mut self.active_turn {
                    turn.consecutive_provider_failures =
                        turn.consecutive_provider_failures.saturating_add(1);
                    turn.provider_retry_allowed = error.retryable;
                }
                self.last_step_failure = Some(StepFailureState {
                    purpose,
                    error: error.clone(),
                });
                self.pending_notices.push(format!(
                    "Provider step failed at {}: {}",
                    error.stage, error.message
                ));
            }
            SessionEvent::StepInterrupted {
                step_id, reason, ..
            } => {
                self.close_step(step_id, "was interrupted");
                match reason {
                    super::events::StepInterruptionReason::Recovery => {
                        self.pending_notices.push(format!(
                            "Provider step {step_id} was interrupted during runtime recovery because it had no durable outcome."
                        ));
                    }
                    super::events::StepInterruptionReason::HandoffTimeout => {
                        self.pending_notices.push(format!(
                            "Provider step {step_id} was interrupted because context handoff reached its total timeout."
                        ));
                    }
                    super::events::StepInterruptionReason::TurnCancelled => {}
                }
            }
            SessionEvent::AutoWaitEnded {
                step_id, reason, ..
            } => {
                if self
                    .auto_wait
                    .as_ref()
                    .is_some_and(|wait| wait.step_id == *step_id)
                {
                    self.auto_wait = None;
                } else {
                    self.diagnose(format!(
                        "Auto wait for step {step_id} ended without a matching active wait."
                    ));
                }
                if *reason == AutoWaitEndReason::TimedOut {
                    self.pending_notices.push(format!(
                        "The automatic wait for tool batch {step_id} timed out."
                    ));
                }
            }
            SessionEvent::ToolCancelRequested { invocation_id, .. } => {
                if let Some(pending) = self.pending_tools.get_mut(invocation_id) {
                    pending.cancel_requested = true;
                } else {
                    self.diagnose(format!(
                        "A cancellation referred to unknown tool invocation {invocation_id}."
                    ));
                }
            }
            SessionEvent::ToolResult { result } => self.apply_tool_result(result, tools),
            SessionEvent::HandoffFailed { message, .. } => {
                self.pending_notices.push(format!(
                    "Context handoff completed without a handoff document: {message}"
                ));
            }
            SessionEvent::HandoffApplied {
                generation,
                document,
                tools,
                carried_tools,
                ..
            } => self.apply_handoff(*generation, document, tools, carried_tools),
            SessionEvent::DeadlineReached {
                deadline,
                reached_at_ms,
            } => self.apply_deadline(deadline, *reached_at_ms),
            SessionEvent::RuntimeFault {
                failure,
                consecutive_count,
                ..
            } => {
                self.fault_streak = Some(FaultStreak {
                    fingerprint: failure.fingerprint(),
                    count: *consecutive_count,
                    completed_steps_after_fault: 0,
                });
                self.pending_notices.push(format!(
                    "Agent runtime failure in {}: {} (consecutive occurrence {}). Tell the user that execution was abnormal; if this continues, help them decide how to proceed.",
                    failure.stage, failure.message, consecutive_count
                ));
            }
            SessionEvent::Snapshot { .. } => {}
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_created(
        &mut self,
        session_id: &str,
        created_at_ms: i64,
        selection: &Selection,
        system_prompt: &Option<String>,
        workspace: &str,
        tools: &[ToolIntroduction],
    ) -> Result<(), FoldError> {
        if self.is_created() {
            self.diagnose("A duplicate SessionCreated event was ignored.");
            return Ok(());
        }
        if session_id != self.session_id {
            return Err(FoldError::SessionIdentity {
                expected: self.session_id.clone(),
                actual: session_id.to_owned(),
            });
        }
        self.created_at_ms = Some(created_at_ms);
        self.selection = Some(selection.clone());
        self.system_prompt = system_prompt.clone();
        self.workspace = workspace.to_owned();
        self.generation = GenerationState::new(1, None, tools.to_vec());
        self.known_tools = catalog_versions(tools);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_step_started(
        &mut self,
        step_id: &str,
        turn_id: &str,
        purpose: Purpose,
        consumed_input_ids: &[String],
        deliveries: &[ToolDelivery],
        tool_changes: &[ToolChange],
        notices: &[String],
        outstanding: &[OutstandingItem],
        max_output_tokens: Option<u32>,
        started_at_ms: i64,
    ) {
        if self.active_step.is_some() {
            self.diagnose("A provider step started while another step was active.");
        }
        if self
            .active_turn
            .as_ref()
            .is_some_and(|turn| turn.turn_id != turn_id)
        {
            self.diagnose(format!(
                "Step {step_id} refers to turn {turn_id}, which is not the active turn."
            ));
        }

        if !tool_changes.is_empty() {
            for change in tool_changes {
                apply_tool_change(&mut self.known_tools, change);
            }
            self.generation.entries.push(GenerationEntry::ToolChanges {
                changes: tool_changes.to_vec(),
            });
        }

        for notice in notices {
            if let Some(index) = self
                .pending_notices
                .iter()
                .position(|pending| pending == notice)
            {
                self.pending_notices.remove(index);
            }
            self.generation.entries.push(GenerationEntry::Notice {
                message: notice.clone(),
            });
        }

        let mut consumed = Vec::with_capacity(consumed_input_ids.len());
        for input_id in consumed_input_ids {
            if let Some(index) = self
                .unconsumed_inputs
                .iter()
                .position(|input| input.input_id == *input_id)
            {
                consumed.push(self.unconsumed_inputs.remove(index));
            } else {
                self.diagnose(format!(
                    "Step {step_id} claimed unknown mailbox input {input_id}."
                ));
            }
        }
        if !consumed.is_empty() {
            self.wait_deadline = None;
            self.generation.entries.push(GenerationEntry::Inputs {
                inputs: consumed.clone(),
            });
        }

        for delivery in deliveries {
            match delivery {
                ToolDelivery::Pending { invocation } => {
                    if let Some(pending) = self.pending_tools.get_mut(&invocation.invocation_id) {
                        pending.delivery = ToolDeliveryState::PendingSent;
                    }
                }
                ToolDelivery::Result { invocation, .. } => {
                    self.pending_tools.remove(&invocation.invocation_id);
                }
            }
            self.generation.entries.push(GenerationEntry::ToolDelivery {
                delivery: delivery.clone(),
            });
        }

        if !outstanding.is_empty() {
            self.generation.entries.push(GenerationEntry::Outstanding {
                items: outstanding.to_vec(),
            });
        }

        self.active_step = Some(ActiveStep {
            step_id: step_id.to_owned(),
            turn_id: turn_id.to_owned(),
            purpose,
            selection: self.selection.clone(),
            request_entries: self.generation.entries.len(),
            consumed_inputs: consumed,
            max_output_tokens,
            started_at_ms,
        });
        if purpose == Purpose::Handoff {
            if let Some(turn) = &mut self.active_turn {
                match &mut turn.handoff {
                    Some(progress) => {
                        progress.attempts = progress.attempts.saturating_add(1);
                    }
                    None => {
                        turn.handoff = Some(HandoffProgress {
                            started_at_ms,
                            attempts: 1,
                        });
                    }
                }
            }
        }
        self.last_step_failure = None;
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_step_completed(
        &mut self,
        step_id: &str,
        assistant_text: &str,
        provider_calls: &[ProviderToolCall],
        invocations: &[ToolInvocation],
        auto_wait_deadline_ms: Option<i64>,
        usage: &Option<Usage>,
        provider_context: &Option<ProviderContext>,
        _completed_at_ms: i64,
    ) {
        let active_step = self
            .active_step
            .as_ref()
            .filter(|step| step.step_id == step_id);
        let request_entries =
            active_step.map_or(self.generation.entries.len(), |step| step.request_entries);
        let selection_is_current =
            active_step.and_then(|step| step.selection.as_ref()) == self.selection.as_ref();
        self.close_step(step_id, "completed");

        if provider_calls.len() != invocations.len() {
            self.diagnose(format!(
                "Step {step_id} persisted {} provider calls and {} logical invocations.",
                provider_calls.len(),
                invocations.len()
            ));
        }
        self.generation.entries.push(GenerationEntry::Assistant {
            step_id: step_id.to_owned(),
            text: assistant_text.to_owned(),
            provider_calls: provider_calls.to_vec(),
            invocations: invocations.to_vec(),
            usage: usage.clone(),
            provider_context: provider_context.clone(),
        });

        for invocation in invocations {
            if self.pending_tools.contains_key(&invocation.invocation_id) {
                self.diagnose(format!(
                    "Step {step_id} reused tool invocation ID {}.",
                    invocation.invocation_id
                ));
                continue;
            }
            self.pending_tools.insert(
                invocation.invocation_id.clone(),
                PendingTool {
                    invocation: invocation.clone(),
                    delivery: ToolDeliveryState::Fresh,
                    result: None,
                    cancel_requested: false,
                },
            );
        }

        self.auto_wait = auto_wait_deadline_ms
            .filter(|_| !invocations.is_empty())
            .map(|deadline| AutoWait {
                step_id: step_id.to_owned(),
                invocation_ids: invocations
                    .iter()
                    .map(|invocation| invocation.invocation_id.clone())
                    .collect(),
                deadline_ms: deadline,
            });

        if selection_is_current {
            if let Some(usage) = usage {
                self.token_anchor = Some(TokenAnchor {
                    input_tokens: usage.input_tokens,
                    generation: self.generation.number,
                    entries: request_entries,
                });
            }
        }
        if let Some(turn) = &mut self.active_turn {
            turn.consecutive_provider_failures = 0;
            turn.provider_retry_allowed = true;
        }
        self.last_step_failure = None;
        if let Some(streak) = &mut self.fault_streak {
            streak.completed_steps_after_fault =
                streak.completed_steps_after_fault.saturating_add(1);
            if streak.completed_steps_after_fault >= 2 {
                self.fault_streak = None;
            }
        }
    }

    fn close_step(&mut self, step_id: &str, verb: &str) {
        match self.active_step.take() {
            Some(step) if step.step_id == step_id => {}
            Some(step) => {
                self.active_step = Some(step);
                self.diagnose(format!(
                    "Step {step_id} {verb}, but a different provider step was active."
                ));
            }
            None => self.diagnose(format!(
                "Step {step_id} {verb} without a matching active provider step."
            )),
        }
    }

    fn apply_tool_result(&mut self, result: &ToolResultData, tools: &ToolRegistry) {
        self.apply_tool_knowledge(result.knowledge.as_ref());
        self.fold_tool_state(result, tools);

        match self.pending_tools.get_mut(&result.invocation_id) {
            Some(pending) if pending.result.is_none() => {
                pending.result = Some(result.clone());
            }
            _ => self.pending_notices.push(format!(
                "Tool result {} arrived for invocation {} but did not match a currently pending invocation. Tool: {}. Outcome: {:?}. Message: {}",
                result.result_id, result.invocation_id, result.tool, result.outcome, result.message
            )),
        }

        if result.tool == WAIT_TOOL_NAME && result.outcome == super::events::ToolOutcome::Succeeded
        {
            if let Some(deadline_ms) = result.data.get("until_ms").and_then(Value::as_i64) {
                self.wait_deadline = Some(WaitDeadline {
                    invocation_id: result.invocation_id.clone(),
                    deadline_ms,
                });
            }
        }
    }

    fn apply_tool_knowledge(&mut self, knowledge: Option<&ToolKnowledge>) {
        match knowledge {
            Some(ToolKnowledge::Current { name, version }) => {
                self.known_tools.insert(name.clone(), version.clone());
            }
            Some(ToolKnowledge::Removed { name }) => {
                self.known_tools.remove(name);
            }
            None => {}
        }
    }

    fn fold_tool_state(&mut self, result: &ToolResultData, tools: &ToolRegistry) {
        let Some(compatibility) = tools.compatibility(&result.tool) else {
            self.diagnose(format!(
                "No compatibility logic is available for tool result {} from {}.",
                result.result_id, result.tool
            ));
            return;
        };
        let migrated_result =
            match compatibility.migrate_result(result.result_schema_version, result.data.clone()) {
                Ok(result) => result,
                Err(error) => {
                    self.diagnose(format!(
                        "Tool {} result {} could not be migrated: {error}",
                        result.tool, result.result_id
                    ));
                    return;
                }
            };
        let current = match self.tool_states.get(&result.tool).cloned() {
            Some(state) => match compatibility.migrate_state(state) {
                Ok(state) => Some(state),
                Err(error) => {
                    self.diagnose(format!(
                        "Tool {} state could not be migrated before result {}: {error}",
                        result.tool, result.result_id
                    ));
                    return;
                }
            },
            None => compatibility.initial_state(),
        };
        match compatibility.fold(current.as_ref(), &migrated_result) {
            Ok(Some(next)) => {
                self.tool_states.insert(result.tool.clone(), next);
            }
            Ok(None) => {
                self.tool_states.remove(&result.tool);
            }
            Err(error) => self.diagnose(format!(
                "Tool {} result {} could not be folded: {error}",
                result.tool, result.result_id
            )),
        }
    }

    fn apply_handoff(
        &mut self,
        generation: u64,
        document: &Option<String>,
        tools: &[ToolIntroduction],
        carried_tools: &[ToolInvocation],
    ) {
        if generation <= self.generation.number {
            self.diagnose(format!(
                "Handoff selected generation {generation} after generation {}.",
                self.generation.number
            ));
        }

        let carried_ids: BTreeSet<_> = carried_tools
            .iter()
            .map(|invocation| invocation.invocation_id.as_str())
            .collect();
        self.pending_tools
            .retain(|id, _| carried_ids.contains(id.as_str()));
        for invocation in carried_tools {
            self.pending_tools
                .entry(invocation.invocation_id.clone())
                .and_modify(|pending| pending.delivery = ToolDeliveryState::PendingSent)
                .or_insert_with(|| PendingTool {
                    invocation: invocation.clone(),
                    delivery: ToolDeliveryState::PendingSent,
                    result: None,
                    cancel_requested: false,
                });
        }

        self.generation = GenerationState::new(generation, document.clone(), tools.to_vec());
        if !carried_tools.is_empty() {
            self.generation.entries.push(GenerationEntry::CarriedTools {
                invocations: carried_tools.to_vec(),
            });
        }
        self.known_tools = catalog_versions(tools);
        self.active_step = None;
        self.auto_wait = None;
        self.token_anchor = None;
        self.last_step_failure = None;
        if let Some(turn) = &mut self.active_turn {
            turn.consecutive_provider_failures = 0;
            turn.provider_retry_allowed = true;
            turn.handoff = None;
        }
    }

    fn apply_deadline(&mut self, deadline: &DeadlineKind, reached_at_ms: i64) {
        match deadline {
            DeadlineKind::Wait { invocation_id } => {
                if self
                    .wait_deadline
                    .as_ref()
                    .is_some_and(|wait| wait.invocation_id == *invocation_id)
                {
                    self.wait_deadline = None;
                    self.pending_notices.push(format!(
                        "Wait for invocation {invocation_id} reached its deadline at {reached_at_ms}."
                    ));
                } else {
                    self.diagnose(format!(
                        "A stale wait deadline fired for invocation {invocation_id}."
                    ));
                }
            }
            DeadlineKind::AutoWait { step_id } => {
                self.pending_notices.push(format!(
                    "The automatic tool wait deadline for step {step_id} was reached at {reached_at_ms}."
                ));
            }
        }
    }

    fn diagnose(&mut self, message: impl Into<String>) {
        self.pending_notices
            .push(format!("Recovered runtime anomaly: {}", message.into()));
    }

    pub fn planned_deliveries(&self, include_pending: bool) -> Vec<ToolDelivery> {
        self.pending_tools
            .values()
            .filter_map(|pending| match (&pending.result, pending.delivery) {
                (Some(result), ToolDeliveryState::Fresh) => Some(ToolDelivery::Result {
                    invocation: pending.invocation.clone(),
                    result: Box::new(result.clone()),
                    mode: ToolDeliveryMode::Direct,
                }),
                (Some(result), ToolDeliveryState::PendingSent) => Some(ToolDelivery::Result {
                    invocation: pending.invocation.clone(),
                    result: Box::new(result.clone()),
                    mode: ToolDeliveryMode::Notification,
                }),
                (None, ToolDeliveryState::Fresh) if include_pending => {
                    Some(ToolDelivery::Pending {
                        invocation: pending.invocation.clone(),
                    })
                }
                _ => None,
            })
            .collect()
    }

    pub fn core_outstanding(&self) -> Vec<OutstandingItem> {
        let mut items = Vec::new();
        if let Some(step) = &self.active_step {
            items.push(OutstandingItem {
                kind: "provider_step".into(),
                id: step.step_id.clone(),
                summary: "A provider step has no durable terminal event.".into(),
            });
        }
        for pending in self
            .pending_tools
            .values()
            .filter(|pending| pending.result.is_none())
        {
            items.push(OutstandingItem {
                kind: "tool".into(),
                id: pending.invocation.invocation_id.clone(),
                summary: format!(
                    "Tool {} has not returned a final result.",
                    pending.invocation.tool
                ),
            });
        }
        for input in &self.unconsumed_inputs {
            items.push(OutstandingItem {
                kind: "mailbox_input".into(),
                id: input.input_id.clone(),
                summary: "Mailbox input has not yet been sent to the agent.".into(),
            });
        }
        items
    }

    pub fn outstanding(&self, tools: &ToolRegistry) -> Vec<OutstandingItem> {
        let mut items = self.core_outstanding();
        for (name, state) in &self.tool_states {
            if let Some(compatibility) = tools.compatibility(name) {
                items.extend(compatibility.outstanding(Some(state)));
            }
        }
        items
    }

    pub fn latest_assistant(&self) -> Option<(&str, &[ToolInvocation])> {
        self.generation.entries.iter().rev().find_map(|entry| {
            if let GenerationEntry::Assistant {
                text, invocations, ..
            } = entry
            {
                Some((text.as_str(), invocations.as_slice()))
            } else {
                None
            }
        })
    }

    pub fn pending(&self, invocation_id: &str) -> Option<&PendingTool> {
        self.pending_tools.get(invocation_id)
    }

    pub fn anchor(&self) -> Option<&TokenAnchor> {
        self.token_anchor
            .as_ref()
            .filter(|anchor| anchor.generation == self.generation.number)
    }
}

fn catalog_versions(catalog: &[ToolIntroduction]) -> BTreeMap<String, ToolVersion> {
    catalog
        .iter()
        .map(|tool| (tool.name.clone(), tool.version.clone()))
        .collect()
}

fn apply_tool_change(known: &mut BTreeMap<String, ToolVersion>, change: &ToolChange) {
    match change {
        ToolChange::Added { name, version } | ToolChange::Updated { name, version } => {
            known.insert(name.clone(), version.clone());
        }
        ToolChange::Removed { name } => {
            known.remove(name);
        }
    }
}

pub fn snapshot_value(state: &SessionState) -> Result<Value, serde_json::Error> {
    serde_json::to_value(state)
}

pub fn migrate_snapshot(
    state_schema_version: u32,
    state: Value,
    tools: &ToolRegistry,
) -> Result<SessionState, SnapshotMigrationError> {
    let mut state: SessionState = match state_schema_version {
        STATE_SCHEMA_VERSION => serde_json::from_value(state)
            .map_err(|error| SnapshotMigrationError::Invalid(error.to_string()))?,
        other => return Err(SnapshotMigrationError::Unsupported(other)),
    };

    let names: Vec<_> = state.tool_states.keys().cloned().collect();
    for name in names {
        let compatibility = tools
            .compatibility(&name)
            .ok_or_else(|| SnapshotMigrationError::MissingTool(name.clone()))?;
        let current = state
            .tool_states
            .remove(&name)
            .expect("tool state name was collected from this map");
        let migrated = compatibility.migrate_state(current).map_err(|message| {
            SnapshotMigrationError::Tool {
                name: name.clone(),
                message,
            }
        })?;
        state.tool_states.insert(name, migrated);
    }
    Ok(state)
}

#[derive(Debug, thiserror::Error)]
pub enum FoldError {
    #[error("session stream does not start with SessionCreated")]
    MissingSessionCreated,
    #[error("session identity mismatch: expected {expected}, event contains {actual}")]
    SessionIdentity { expected: String, actual: String },
}

#[derive(Debug, thiserror::Error)]
pub enum SnapshotMigrationError {
    #[error("unsupported snapshot state schema version {0}")]
    Unsupported(u32),
    #[error("invalid snapshot state: {0}")]
    Invalid(String),
    #[error("snapshot requires missing tool compatibility logic for {0}")]
    MissingTool(String),
    #[error("snapshot tool state migration failed for {name}: {message}")]
    Tool { name: String, message: String },
}
