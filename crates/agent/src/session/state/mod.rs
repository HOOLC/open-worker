use std::{collections::BTreeSet, path::Path, sync::Arc};

mod reducer;
pub use reducer::DomainError;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

pub const EVENT_SCHEMA_VERSION: u32 = 11;
pub const STATE_SCHEMA_VERSION: u32 = 14;
pub const REDUCER_SCHEMA_VERSION: u32 = 14;
pub const SESSION_CREATED_SCHEMA_VERSION: u32 = 5;
pub const WAIT_MIN_SECONDS: u32 = 1;
pub const WAIT_MAX_SECONDS: u32 = 600;
pub const WAIT_FOR_TOOL_NAME: &str = "wait_for";
pub const MAX_MODEL_ATTEMPTS_PER_STEP: u32 = 32;

pub type StreamVersion = u64;

/// Durable execution status for the one activation that may own a session.
///
/// The in-memory runtime actor is disposable; this record is the durable
/// fencing fact used by restart reconciliation and by the next round boundary.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivationOutcome {
    Finished,
    Wait,
    Failed,
    Interrupted,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ActiveActivation {
    pub activation_id: String,
    pub selection: SessionSelection,
    pub started_at_ms: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ActiveModelRound {
    pub activation_id: String,
    pub round_id: String,
    #[serde(default, skip_serializing_if = "ModelRequestPurpose::is_conversation")]
    pub purpose: ModelRequestPurpose,
    pub mailbox_through_seq: u64,
    pub started_at_ms: i64,
    #[serde(default)]
    pub request: Option<ModelRequestFact>,
    #[serde(default)]
    pub attempt: Option<ModelAttemptRecord>,
    #[serde(default)]
    pub retry: Option<ModelRetrySchedule>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ModelRequestFact {
    pub activation_id: String,
    pub round_id: String,
    pub request_id: String,
    pub request_fingerprint: String,
    pub prompt_fingerprint: String,
    pub tool_schema_fingerprint: String,
    pub maximum_attempts: u32,
}

/// Exact usage returned by the provider for one completed conversation
/// request. It anchors the next preflight calculation without persisting any
/// request, prompt, transcript, or provider payload.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ModelUsageAnchor {
    pub context_generation: u64,
    pub selection_fingerprint: String,
    pub tool_schema_fingerprint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_event_id: Option<String>,
    pub input_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_input_tokens: Option<u64>,
    pub output_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_reasoning_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_text_tokens: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderInputMode {
    Full,
    Delta,
}

/// Non-sensitive facts about the input actually placed on the provider wire.
/// It deliberately contains neither the input items nor their serialized
/// bytes.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProviderInputDiagnostics {
    pub mode: ProviderInputMode,
    pub logical_input_items: u64,
    pub sent_input_items: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_response_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ModelAttemptRecord {
    pub activation_id: String,
    pub round_id: String,
    pub request_id: String,
    pub attempt_id: String,
    pub attempt_number: u32,
    pub started_at_ms: i64,
    #[serde(default)]
    pub outcome: ModelAttemptOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<ModelAttemptFailureCause>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ModelAttemptFailureCause {
    pub error_class: String,
    pub retryable: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelAttemptOutcome {
    #[default]
    Running,
    Failed,
    Interrupted,
    Completed,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelRequestPurpose {
    #[default]
    Conversation,
    ContextHandoff,
}

impl ModelRequestPurpose {
    fn is_conversation(&self) -> bool {
        *self == Self::Conversation
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ModelRetrySchedule {
    pub activation_id: String,
    pub round_id: String,
    pub request_id: String,
    pub failed_attempt_id: String,
    pub failed_attempt_number: u32,
    pub next_attempt_number: u32,
    pub delay_ms: u64,
    pub not_before_ms: i64,
    pub maximum_attempts: u32,
    pub error_class: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ContextHandoffPlan {
    pub plan_id: String,
    pub activation_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_handoff_id: Option<String>,
    pub next_generation: u64,
    pub covered_through_message_id: String,
    /// Frozen output allowance for the already-declared provider request.
    pub max_output_tokens: u32,
    pub selection: SessionSelection,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ContextHandoffDocument {
    pub handoff_id: String,
    pub plan_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_handoff_id: Option<String>,
    pub next_generation: u64,
    pub covered_through_message_id: String,
    /// Agent-authored document submitted through context_handoff for the next
    /// generation. This is a first-class session fact, not a generic tool
    /// payload envelope.
    pub document: String,
    /// Provider-reported non-reasoning output tokens for the handoff call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document_tokens: Option<u64>,
    pub selection: SessionSelection,
}

/// The current live continuation root kept in the session projection.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ContextHandoffState {
    /// This is the ULID of the immutable ContextHandoffCreated event.
    pub handoff_id: String,
    pub plan_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_handoff_id: Option<String>,
    pub next_generation: u64,
    pub covered_through_message_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document_tokens: Option<u64>,
    pub selection: SessionSelection,
}

impl From<&ContextHandoffDocument> for ContextHandoffState {
    fn from(handoff: &ContextHandoffDocument) -> Self {
        Self {
            handoff_id: handoff.handoff_id.clone(),
            plan_id: handoff.plan_id.clone(),
            previous_handoff_id: handoff.previous_handoff_id.clone(),
            next_generation: handoff.next_generation,
            covered_through_message_id: handoff.covered_through_message_id.clone(),
            document_tokens: handoff.document_tokens,
            selection: handoff.selection.clone(),
        }
    }
}

impl ContextHandoffState {
    pub fn matches_document(&self, handoff: &ContextHandoffDocument) -> bool {
        self.handoff_id == handoff.handoff_id
            && self.plan_id == handoff.plan_id
            && self.previous_handoff_id == handoff.previous_handoff_id
            && self.next_generation == handoff.next_generation
            && self.covered_through_message_id == handoff.covered_through_message_id
            && self.document_tokens == handoff.document_tokens
            && self.selection == handoff.selection
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WaitTimerIntent {
    pub wait_id: String,
    pub deadline_ms: i64,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct SessionSelection {
    pub profile_id: String,
    pub model: String,
    pub thinking: String,
}

impl SessionSelection {
    pub fn validate(&self) -> Result<(), DomainError> {
        validate_identifier("profile_id", &self.profile_id)?;
        validate_identifier("model", &self.model)?;
        validate_identifier("thinking", &self.thinking)
    }
}

/// A provider-neutral continuation that may be carried across model rounds.
///
/// The reducer only validates its envelope. The bytes are opaque to
/// the domain and are never decoded, logged, or used to make an effect
/// decision.  Provider adapters are responsible for constructing and
/// interpreting a value after it has been admitted by the application layer.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OpaqueContinuation {
    pub provider_type: String,
    pub codec_version: u32,
    pub semantic_kind: String,
    pub bytes: Vec<u8>,
}

impl OpaqueContinuation {
    pub fn new(
        provider_type: impl Into<String>,
        codec_version: u32,
        semantic_kind: impl Into<String>,
        bytes: Vec<u8>,
    ) -> Result<Self, DomainError> {
        let continuation = Self {
            provider_type: provider_type.into(),
            codec_version,
            semantic_kind: semantic_kind.into(),
            bytes,
        };
        continuation.validate()?;
        Ok(continuation)
    }

    pub fn validate(&self) -> Result<(), DomainError> {
        validate_identifier("continuation provider type", &self.provider_type)?;
        if self.codec_version == 0 {
            return Err(DomainError::InvalidState(
                "continuation codec version must be positive".into(),
            ));
        }
        validate_identifier("continuation semantic kind", &self.semantic_kind)?;
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptRole {
    System,
    User,
    Assistant,
    Tool,
}

impl TranscriptRole {
    pub fn is_mailbox_input(&self) -> bool {
        self == &Self::User
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ToolCall {
    pub tool_call_id: String,
    pub tool_name: String,
    pub arguments: Value,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ProviderContext {
    pub profile_id: String,
    pub provider: String,
    pub model: String,
    pub api: String,
    pub output_items: Arc<Vec<Value>>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct TranscriptMessage {
    pub message_id: String,
    pub role: TranscriptRole,
    pub content: Arc<str>,
    pub is_error: bool,
    #[serde(default)]
    pub tool_call_id: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_context: Option<ProviderContext>,
    #[serde(default)]
    pub source_mailbox_seq: Option<u64>,
}

/// Provider-facing projection. It is not a durable session message and has no
/// message identity of its own.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ProviderMessage {
    pub role: TranscriptRole,
    pub content: Arc<str>,
    pub is_error: bool,
    #[serde(default)]
    pub tool_call_id: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_context: Option<ProviderContext>,
}

impl From<&TranscriptMessage> for ProviderMessage {
    fn from(message: &TranscriptMessage) -> Self {
        Self {
            role: message.role.clone(),
            content: message.content.clone(),
            is_error: message.is_error,
            tool_call_id: message.tool_call_id.clone(),
            tool_calls: message.tool_calls.clone(),
            provider_context: message.provider_context.clone(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct MailboxMessage {
    pub message_id: String,
    pub mailbox_seq: u64,
    pub content: Arc<str>,
    pub received_at_ms: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WaitSource {
    WaitFor,
    AutoToolBatch,
    Runtime,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ActiveWait {
    pub wait_id: String,
    pub reason: String,
    pub timeout_seconds: u32,
    pub deadline_ms: i64,
    pub source: WaitSource,
    #[serde(default)]
    pub tool_call_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelAttemptErrorClass {
    ProviderUnavailable,
    InvalidSelection,
    ProfileUnavailable,
    ProviderFailed,
    InvalidToolArguments,
    ContextHandoffFailed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ModelAttemptError {
    pub class: ModelAttemptErrorClass,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ModelAttemptFailure {
    pub trigger_message_id: String,
    pub error: ModelAttemptError,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ModelAttemptsExhaustedFact {
    pub activation_id: String,
    pub round_id: String,
    pub request_id: String,
    pub attempt_id: String,
    pub attempt_number: u32,
    pub maximum_attempts: u32,
    pub finished_at_ms: i64,
}

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum SessionEvent {
    SessionCreated {
        schema_version: u32,
        session_id: String,
        created_at_ms: i64,
        selection: SessionSelection,
        system_prompt: Option<String>,
        workspace: String,
    },
    SelectionChanged {
        selection: SessionSelection,
    },
    MailboxMessageAppended {
        message: MailboxMessage,
    },
    MailboxDrained {
        through_mailbox_seq: u64,
    },
    MessageAppended {
        message: TranscriptMessage,
        #[serde(default)]
        wake_wait: bool,
    },
    /// Claims one session activation and freezes the selection used by all
    /// rounds in that activation.
    ActivationStarted {
        activation_id: String,
        selection: SessionSelection,
        started_at_ms: i64,
    },
    ModelRoundStarted {
        activation_id: String,
        round_id: String,
        #[serde(default)]
        purpose: ModelRequestPurpose,
        mailbox_through_seq: u64,
        started_at_ms: i64,
    },
    ContextHandoffPlanned {
        plan: ContextHandoffPlan,
    },
    ContextHandoffCreated {
        handoff: ContextHandoffDocument,
    },
    /// The model did not submit the one valid context_handoff call requested
    /// by the runtime. The exact output is durable evidence but is never
    /// projected into live model context and none of its tools are executed.
    ContextHandoffRejected {
        plan_id: String,
        assistant_content: String,
        tool_calls: Vec<ToolCall>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_context: Option<ProviderContext>,
    },
    ContextHandoffFailed {
        plan_id: String,
        error: ModelAttemptError,
        finished_at_ms: i64,
    },
    ModelRequestDeclared {
        activation_id: String,
        round_id: String,
        request_id: String,
        request_fingerprint: String,
        prompt_fingerprint: String,
        tool_schema_fingerprint: String,
        maximum_attempts: u32,
    },
    ModelAttemptStarted {
        activation_id: String,
        round_id: String,
        request_id: String,
        attempt_id: String,
        attempt_number: u32,
        started_at_ms: i64,
    },
    /// Provider-attempt lifecycle failure fact. `ModelAttemptFailed` below is
    /// the terminal user-input failure projected after retries are exhausted.
    ModelAttemptFailedFact {
        activation_id: String,
        round_id: String,
        request_id: String,
        attempt_id: String,
        attempt_number: u32,
        error_class: String,
        retryable: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_input: Option<ProviderInputDiagnostics>,
    },
    ModelAttemptInterrupted {
        activation_id: String,
        round_id: String,
        request_id: String,
        attempt_id: String,
        attempt_number: u32,
        reason: String,
    },
    ModelRequestAbandoned {
        activation_id: String,
        round_id: String,
        request_id: String,
        attempt_id: String,
        reason: String,
        abandoned_at_ms: i64,
    },
    ModelAttemptsExhausted {
        fact: ModelAttemptsExhaustedFact,
    },
    ModelStepRetryScheduled {
        schedule: ModelRetrySchedule,
    },
    ModelRequestCompleted {
        activation_id: String,
        round_id: String,
        request_id: String,
        attempt_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<ModelUsageAnchor>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_input: Option<ProviderInputDiagnostics>,
    },
    ActivationFinished {
        activation_id: String,
        outcome: ActivationOutcome,
        finished_at_ms: i64,
    },
    ModelAttemptFailed {
        failure: ModelAttemptFailure,
    },
    WaitSet {
        wait: ActiveWait,
    },
    WaitTimerScheduled {
        timer: WaitTimerIntent,
    },
    WaitCleared {
        wait_id: String,
    },
    WaitExpired {
        wait_id: String,
    },
}

impl SessionEvent {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::SessionCreated { .. } => "session_created",
            Self::SelectionChanged { .. } => "selection_changed",
            Self::MailboxMessageAppended { .. } => "mailbox_message_appended",
            Self::MailboxDrained { .. } => "mailbox_drained",
            Self::MessageAppended { .. } => "message_appended",
            Self::ActivationStarted { .. } => "activation_started",
            Self::ModelRoundStarted { .. } => "model_round_started",
            Self::ContextHandoffPlanned { .. } => "context_handoff_planned",
            Self::ContextHandoffCreated { .. } => "context_handoff_created",
            Self::ContextHandoffRejected { .. } => "context_handoff_rejected",
            Self::ContextHandoffFailed { .. } => "context_handoff_failed",
            Self::ModelRequestDeclared { .. } => "model_request_declared",
            Self::ModelAttemptStarted { .. } => "model_attempt_started",
            Self::ModelAttemptFailedFact { .. } => "model_attempt_failed",
            Self::ModelAttemptInterrupted { .. } => "model_attempt_interrupted",
            Self::ModelRequestAbandoned { .. } => "model_request_abandoned",
            Self::ModelAttemptsExhausted { .. } => "model_attempts_exhausted",
            Self::ModelStepRetryScheduled { .. } => "model_step_retry_scheduled",
            Self::ModelRequestCompleted { .. } => "model_request_completed",
            Self::ActivationFinished { .. } => "activation_finished",
            Self::ModelAttemptFailed { .. } => "model_attempt_failed",
            Self::WaitSet { .. } => "wait_set",
            Self::WaitTimerScheduled { .. } => "wait_timer_scheduled",
            Self::WaitCleared { .. } => "wait_cleared",
            Self::WaitExpired { .. } => "wait_expired",
        }
    }

    /// Identity of the durable object created by this fact.
    ///
    /// A created object never receives a second identifier: its identity is
    /// the enclosing EventRecord ULID. Follow-up facts return `None` because
    /// their Event ULID identifies only the fact itself.
    pub(crate) fn created_object_id(&self) -> Option<&str> {
        match self {
            Self::SessionCreated { session_id, .. } => Some(session_id),
            Self::MailboxMessageAppended { message } => Some(&message.message_id),
            Self::MessageAppended { message, .. } => Some(&message.message_id),
            Self::ActivationStarted { activation_id, .. } => Some(activation_id),
            Self::ModelRoundStarted { round_id, .. } => Some(round_id),
            Self::ContextHandoffPlanned { plan } => Some(&plan.plan_id),
            Self::ContextHandoffCreated { handoff } => Some(&handoff.handoff_id),
            Self::ModelRequestDeclared { request_id, .. } => Some(request_id),
            Self::ModelAttemptStarted { attempt_id, .. } => Some(attempt_id),
            Self::WaitSet { wait } => Some(&wait.wait_id),
            _ => None,
        }
    }

    pub fn validate(&self) -> Result<(), DomainError> {
        match self {
            Self::SessionCreated {
                schema_version,
                session_id,
                created_at_ms,
                selection,
                system_prompt: _,
                workspace,
            } => {
                if *schema_version != SESSION_CREATED_SCHEMA_VERSION {
                    return Err(DomainError::UnsupportedSessionCreatedSchema(
                        *schema_version,
                    ));
                }
                validate_identifier("session_id", session_id)?;
                if *created_at_ms < 0 {
                    return Err(DomainError::InvalidCreatedAt);
                }
                selection.validate()?;
                validate_workspace(workspace)?;
            }
            Self::SelectionChanged { selection } => selection.validate()?,
            Self::MailboxMessageAppended { message } => validate_mailbox_message(message)?,
            Self::MailboxDrained {
                through_mailbox_seq,
            } => {
                if *through_mailbox_seq == 0 {
                    return Err(DomainError::InvalidState(
                        "mailbox sequence starts at one".into(),
                    ));
                }
            }
            Self::MessageAppended { message, .. } => validate_message(message)?,
            Self::ActivationStarted {
                activation_id,
                selection,
                started_at_ms,
            } => {
                validate_identifier("activation_id", activation_id)?;
                selection.validate()?;
                validate_non_negative_timestamp("activation started_at_ms", *started_at_ms)?;
            }
            Self::ModelRoundStarted {
                activation_id,
                round_id,
                purpose: _,
                mailbox_through_seq: _,
                started_at_ms,
            } => {
                validate_identifier("activation_id", activation_id)?;
                validate_identifier("round_id", round_id)?;
                validate_non_negative_timestamp("model round started_at_ms", *started_at_ms)?;
            }
            Self::ContextHandoffPlanned { plan } => {
                validate_context_handoff_plan(plan)?;
            }
            Self::ContextHandoffCreated { handoff } => {
                validate_context_handoff_document(handoff)?;
            }
            Self::ContextHandoffRejected {
                plan_id,
                tool_calls,
                provider_context,
                ..
            } => {
                validate_identifier("context handoff plan_id", plan_id)?;
                validate_tool_calls(None, tool_calls)?;
                if let Some(context) = provider_context {
                    validate_provider_context(context)?;
                }
            }
            Self::ContextHandoffFailed {
                plan_id,
                error,
                finished_at_ms,
            } => {
                validate_identifier("context handoff plan_id", plan_id)?;
                validate_model_error(error)?;
                validate_non_negative_timestamp("context handoff finished_at_ms", *finished_at_ms)?;
            }
            Self::ModelRequestDeclared {
                activation_id,
                round_id,
                request_id,
                request_fingerprint,
                prompt_fingerprint,
                tool_schema_fingerprint,
                maximum_attempts,
            } => {
                validate_identifier("activation_id", activation_id)?;
                validate_identifier("round_id", round_id)?;
                validate_identifier("request_id", request_id)?;
                validate_model_fingerprint("request_fingerprint", request_fingerprint)?;
                validate_model_fingerprint("prompt_fingerprint", prompt_fingerprint)?;
                validate_model_fingerprint("tool_schema_fingerprint", tool_schema_fingerprint)?;
                if *maximum_attempts == 0 || *maximum_attempts > MAX_MODEL_ATTEMPTS_PER_STEP {
                    return Err(DomainError::InvalidState(
                        "model request maximum attempts are outside the bounded range".into(),
                    ));
                }
            }
            Self::ModelAttemptStarted {
                activation_id,
                round_id,
                request_id,
                attempt_id,
                attempt_number,
                started_at_ms,
            } => {
                validate_identifier("activation_id", activation_id)?;
                validate_identifier("round_id", round_id)?;
                validate_identifier("request_id", request_id)?;
                validate_identifier("attempt_id", attempt_id)?;
                if *attempt_number == 0 || *attempt_number > MAX_MODEL_ATTEMPTS_PER_STEP {
                    return Err(DomainError::InvalidState(
                        "model attempt number is outside the bounded range".into(),
                    ));
                }
                validate_non_negative_timestamp("model attempt started_at_ms", *started_at_ms)?;
            }
            Self::ModelAttemptFailedFact {
                activation_id,
                round_id,
                request_id,
                attempt_id,
                attempt_number,
                error_class,
                provider_input,
                ..
            } => {
                validate_identifier("activation_id", activation_id)?;
                validate_identifier("round_id", round_id)?;
                validate_identifier("request_id", request_id)?;
                validate_identifier("attempt_id", attempt_id)?;
                validate_identifier("model attempt error class", error_class)?;
                if *attempt_number == 0 || *attempt_number > MAX_MODEL_ATTEMPTS_PER_STEP {
                    return Err(DomainError::InvalidState(
                        "model attempt number is outside the bounded range".into(),
                    ));
                }
                if let Some(provider_input) = provider_input {
                    validate_provider_input_diagnostics(provider_input)?;
                }
            }
            Self::ModelAttemptInterrupted {
                activation_id,
                round_id,
                request_id,
                attempt_id,
                attempt_number,
                reason,
            } => {
                validate_identifier("activation_id", activation_id)?;
                validate_identifier("round_id", round_id)?;
                validate_identifier("request_id", request_id)?;
                validate_identifier("attempt_id", attempt_id)?;
                validate_bounded_text("model interruption reason", reason)?;
                if *attempt_number == 0 || *attempt_number > MAX_MODEL_ATTEMPTS_PER_STEP {
                    return Err(DomainError::InvalidState(
                        "model attempt number is outside the bounded range".into(),
                    ));
                }
            }
            Self::ModelRequestAbandoned {
                activation_id,
                round_id,
                request_id,
                attempt_id,
                reason,
                abandoned_at_ms,
            } => {
                validate_identifier("activation_id", activation_id)?;
                validate_identifier("round_id", round_id)?;
                validate_identifier("request_id", request_id)?;
                validate_identifier("attempt_id", attempt_id)?;
                validate_bounded_text("model request abandonment reason", reason)?;
                validate_non_negative_timestamp("model request abandoned_at_ms", *abandoned_at_ms)?;
            }
            Self::ModelAttemptsExhausted { fact } => {
                validate_model_attempts_exhausted(fact)?;
            }
            Self::ModelStepRetryScheduled { schedule } => validate_retry_schedule(schedule)?,
            Self::ModelRequestCompleted {
                activation_id,
                round_id,
                request_id,
                attempt_id,
                usage,
                provider_input,
            } => {
                validate_identifier("activation_id", activation_id)?;
                validate_identifier("round_id", round_id)?;
                validate_identifier("request_id", request_id)?;
                validate_identifier("attempt_id", attempt_id)?;
                if let Some(usage) = usage {
                    validate_model_usage_anchor(usage)?;
                }
                if let Some(provider_input) = provider_input {
                    validate_provider_input_diagnostics(provider_input)?;
                }
            }
            Self::ActivationFinished {
                activation_id,
                finished_at_ms,
                ..
            } => {
                validate_identifier("activation_id", activation_id)?;
                validate_non_negative_timestamp("activation finished_at_ms", *finished_at_ms)?;
            }
            Self::ModelAttemptFailed { failure } => validate_model_attempt_failure(failure)?,
            Self::WaitSet { wait } => validate_wait(wait)?,
            Self::WaitTimerScheduled { timer } => {
                validate_identifier("wait timer wait_id", &timer.wait_id)?;
                validate_non_negative_timestamp("wait timer deadline_ms", timer.deadline_ms)?;
            }
            Self::WaitCleared { wait_id } | Self::WaitExpired { wait_id } => {
                validate_identifier("wait_id", wait_id)?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EventDraft {
    pub event_id: String,
    pub event: SessionEvent,
}

impl EventDraft {
    pub fn new(event: SessionEvent) -> Self {
        assert!(
            event.created_object_id().is_none(),
            "created objects require EventDraft::identified or EventDraft::batch"
        );
        let event_id = crate::ids::new_ulid();
        Self { event_id, event }
    }

    pub fn identified(build: impl FnOnce(&str) -> SessionEvent) -> Self {
        let event_id = crate::ids::new_ulid();
        let event = build(&event_id);
        assert_eq!(event.created_object_id(), Some(event_id.as_str()));
        Self { event_id, event }
    }

    /// Allocate all Event ULIDs for one atomic append before constructing its
    /// payloads, so events in the same batch can refer to one another without
    /// inventing another identity.
    pub fn batch(
        event_count: usize,
        build: impl FnOnce(&[String]) -> Vec<SessionEvent>,
    ) -> Vec<Self> {
        Self::try_batch(event_count, |ids| {
            Ok::<_, std::convert::Infallible>(build(ids))
        })
        .expect("infallible event batch builder")
    }

    pub fn try_batch<E>(
        event_count: usize,
        build: impl FnOnce(&[String]) -> Result<Vec<SessionEvent>, E>,
    ) -> Result<Vec<Self>, E> {
        assert!(event_count > 0, "event batch cannot be empty");
        let event_ids = (0..event_count)
            .map(|_| crate::ids::new_ulid())
            .collect::<Vec<_>>();
        let events = build(&event_ids)?;
        assert_eq!(
            events.len(),
            event_count,
            "event batch size changed while building"
        );
        Ok(event_ids
            .into_iter()
            .zip(events)
            .map(|(event_id, event)| {
                if let Some(object_id) = event.created_object_id() {
                    assert_eq!(object_id, event_id);
                }
                Self { event_id, event }
            })
            .collect())
    }

    pub fn many(events: Vec<SessionEvent>) -> Vec<Self> {
        events.into_iter().map(Self::new).collect()
    }

    pub fn single(event: SessionEvent) -> Vec<Self> {
        vec![Self::new(event)]
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct EventRecord {
    pub stream_id: String,
    pub stream_version: StreamVersion,
    pub event_id: String,
    pub event_schema_version: u32,
    pub batch_index: u32,
    pub batch_size: u32,
    pub event: SessionEvent,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SessionState {
    pub session_id: String,
    pub created_at_ms: Option<i64>,
    pub selection: SessionSelection,
    pub system_prompt: Option<String>,
    pub workspace: String,
    pub transcript: Vec<TranscriptMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_model_attempt_failure: Option<ModelAttemptFailure>,
    pub mailbox: Vec<MailboxMessage>,
    pub consumed_through_mailbox_seq: u64,
    pub active_wait: Option<ActiveWait>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_timer: Option<WaitTimerIntent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wake_pending_wait_id: Option<String>,
    pub inflight_tool_call_ids: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_activation: Option<ActiveActivation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_activation_outcome: Option<ActivationOutcome>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_model_round: Option<ActiveModelRound>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_context_handoff: Option<ContextHandoffPlan>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_context_handoff: Option<ContextHandoffState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_model_usage: Option<ModelUsageAnchor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_context_handoff_failure: Option<ModelAttemptError>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_model_attempts_exhausted: Option<ModelAttemptsExhaustedFact>,
    pub stream_version: StreamVersion,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DomainDecision {
    pub effective_events: Vec<SessionEvent>,
    pub state: SessionState,
}

fn validate_mailbox_message(message: &MailboxMessage) -> Result<(), DomainError> {
    validate_identifier("mailbox message_id", &message.message_id)?;
    if message.mailbox_seq == 0 {
        return Err(DomainError::InvalidState(
            "mailbox sequence starts at one".into(),
        ));
    }
    require_text("mailbox content", &message.content)?;
    if message.received_at_ms < 0 {
        return Err(DomainError::InvalidTimestamp {
            field: "mailbox received_at_ms",
        });
    }
    Ok(())
}

fn validate_message(message: &TranscriptMessage) -> Result<(), DomainError> {
    validate_identifier("message_id", &message.message_id)?;
    if let Some(tool_call_id) = &message.tool_call_id {
        validate_identifier("tool_call_id", tool_call_id)?;
    }
    if message.role != TranscriptRole::Assistant && message.provider_context.is_some() {
        return Err(DomainError::InvalidState(
            "only assistant messages can carry provider context".into(),
        ));
    }
    if message.source_mailbox_seq == Some(0) {
        return Err(DomainError::InvalidState(
            "message source mailbox sequence starts at one".into(),
        ));
    }
    validate_tool_calls(message.tool_call_id.as_deref(), &message.tool_calls)?;
    if let Some(context) = &message.provider_context {
        validate_provider_context(context)?;
    }
    Ok(())
}

fn validate_tool_calls(
    enclosing_tool_call_id: Option<&str>,
    tool_calls: &[ToolCall],
) -> Result<(), DomainError> {
    let mut tool_call_ids = BTreeSet::new();
    if let Some(tool_call_id) = enclosing_tool_call_id {
        tool_call_ids.insert(tool_call_id);
    }
    for call in tool_calls {
        validate_identifier("tool_call_id", &call.tool_call_id)?;
        validate_identifier("tool_name", &call.tool_name)?;
        if !call.arguments.is_object() {
            return Err(DomainError::InvalidState(
                "tool call arguments must be an object".into(),
            ));
        }
        if !tool_call_ids.insert(call.tool_call_id.as_str()) {
            return Err(DomainError::DuplicateTranscriptToolCallId(
                call.tool_call_id.clone(),
            ));
        }
    }
    Ok(())
}

fn validate_provider_context(context: &ProviderContext) -> Result<(), DomainError> {
    validate_identifier("provider context profile_id", &context.profile_id)?;
    validate_identifier("provider context provider", &context.provider)?;
    validate_identifier("provider context model", &context.model)?;
    validate_identifier("provider context api", &context.api)?;
    if context.output_items.iter().any(|item| !item.is_object()) {
        return Err(DomainError::InvalidState(
            "provider context output items must be objects".into(),
        ));
    }
    Ok(())
}

fn validate_wait(wait: &ActiveWait) -> Result<(), DomainError> {
    validate_identifier("wait_id", &wait.wait_id)?;
    validate_bounded_text("reason", &wait.reason)?;
    if !(WAIT_MIN_SECONDS..=WAIT_MAX_SECONDS).contains(&wait.timeout_seconds) {
        return Err(DomainError::InvalidWaitTimeout);
    }
    if wait.deadline_ms < 0 {
        return Err(DomainError::InvalidTimestamp {
            field: "wait deadline_ms",
        });
    }
    for tool_call_id in &wait.tool_call_ids {
        validate_identifier("tool_call_id", tool_call_id)?;
    }
    Ok(())
}

fn validate_model_attempt_failure(failure: &ModelAttemptFailure) -> Result<(), DomainError> {
    validate_identifier("trigger_message_id", &failure.trigger_message_id)?;
    validate_bounded_text("model error message", &failure.error.message)?;
    validate_model_error(&failure.error)
}

fn validate_model_error(error: &ModelAttemptError) -> Result<(), DomainError> {
    validate_bounded_text("model error message", &error.message)
}

fn validate_model_usage_anchor(anchor: &ModelUsageAnchor) -> Result<(), DomainError> {
    if anchor.context_generation == 0 || anchor.input_tokens == 0 {
        return Err(DomainError::InvalidState(
            "model usage anchor has invalid token accounting".into(),
        ));
    }
    validate_model_fingerprint(
        "model usage selection fingerprint",
        &anchor.selection_fingerprint,
    )?;
    validate_model_fingerprint(
        "model usage tool schema fingerprint",
        &anchor.tool_schema_fingerprint,
    )?;
    if let Some(result_event_id) = &anchor.result_event_id {
        validate_identifier("model usage result_event_id", result_event_id)?;
    }
    Ok(())
}

fn validate_provider_input_diagnostics(
    diagnostics: &ProviderInputDiagnostics,
) -> Result<(), DomainError> {
    if diagnostics.sent_input_items > diagnostics.logical_input_items {
        return Err(DomainError::InvalidState(
            "provider sent input item count exceeds logical input".into(),
        ));
    }
    match diagnostics.mode {
        ProviderInputMode::Full => {
            if diagnostics.previous_response_id.is_some()
                || diagnostics.sent_input_items != diagnostics.logical_input_items
            {
                return Err(DomainError::InvalidState(
                    "full provider input has continuation or omits logical items".into(),
                ));
            }
        }
        ProviderInputMode::Delta => {
            let previous_response_id =
                diagnostics.previous_response_id.as_deref().ok_or_else(|| {
                    DomainError::InvalidState(
                        "delta provider input has no previous response id".into(),
                    )
                })?;
            validate_identifier("provider previous_response_id", previous_response_id)?;
        }
    }
    if let Some(response_id) = diagnostics.response_id.as_deref() {
        validate_identifier("provider response_id", response_id)?;
    }
    Ok(())
}

fn validate_context_handoff_plan(plan: &ContextHandoffPlan) -> Result<(), DomainError> {
    validate_identifier("context handoff plan_id", &plan.plan_id)?;
    validate_identifier("context handoff activation_id", &plan.activation_id)?;
    if let Some(previous_handoff_id) = &plan.previous_handoff_id {
        validate_identifier("previous context handoff_id", previous_handoff_id)?;
    }
    validate_identifier(
        "context handoff covered message_id",
        &plan.covered_through_message_id,
    )?;
    if plan.next_generation < 2 || plan.max_output_tokens == 0 {
        return Err(DomainError::InvalidState(
            "context handoff plan has invalid generation or output allowance".into(),
        ));
    }
    plan.selection.validate()
}

fn validate_context_handoff_document(handoff: &ContextHandoffDocument) -> Result<(), DomainError> {
    validate_identifier("context handoff_id", &handoff.handoff_id)?;
    validate_identifier("context handoff plan_id", &handoff.plan_id)?;
    if let Some(previous_handoff_id) = &handoff.previous_handoff_id {
        validate_identifier("previous context handoff_id", previous_handoff_id)?;
    }
    validate_identifier(
        "context handoff covered message_id",
        &handoff.covered_through_message_id,
    )?;
    require_text("context handoff document", &handoff.document)?;
    if handoff.document_tokens == Some(0) {
        return Err(DomainError::InvalidState(
            "context handoff document has invalid token accounting".into(),
        ));
    }
    if handoff.next_generation < 2 {
        return Err(DomainError::InvalidState(
            "context handoff document has invalid generation".into(),
        ));
    }
    handoff.selection.validate()
}

fn validate_context_handoff_state(handoff: &ContextHandoffState) -> Result<(), DomainError> {
    validate_identifier("context handoff_id", &handoff.handoff_id)?;
    validate_identifier("context handoff plan_id", &handoff.plan_id)?;
    if let Some(previous_handoff_id) = &handoff.previous_handoff_id {
        validate_identifier("previous context handoff_id", previous_handoff_id)?;
    }
    validate_identifier(
        "context handoff covered message_id",
        &handoff.covered_through_message_id,
    )?;
    if handoff.document_tokens == Some(0) {
        return Err(DomainError::InvalidState(
            "context handoff document has invalid token accounting".into(),
        ));
    }
    if handoff.next_generation < 2 {
        return Err(DomainError::InvalidState(
            "context handoff state has invalid generation".into(),
        ));
    }
    handoff.selection.validate()
}

fn validate_non_negative_timestamp(field: &'static str, value: i64) -> Result<(), DomainError> {
    if value < 0 {
        Err(DomainError::InvalidTimestamp { field })
    } else {
        Ok(())
    }
}

fn validate_model_fingerprint(field: &'static str, value: &str) -> Result<(), DomainError> {
    validate_identifier(field, value)
}

fn validate_model_attempts_exhausted(fact: &ModelAttemptsExhaustedFact) -> Result<(), DomainError> {
    validate_identifier("activation_id", &fact.activation_id)?;
    validate_identifier("round_id", &fact.round_id)?;
    validate_identifier("request_id", &fact.request_id)?;
    validate_identifier("attempt_id", &fact.attempt_id)?;
    if fact.attempt_number == 0
        || fact.attempt_number > MAX_MODEL_ATTEMPTS_PER_STEP
        || fact.maximum_attempts == 0
        || fact.maximum_attempts > MAX_MODEL_ATTEMPTS_PER_STEP
        || fact.attempt_number != fact.maximum_attempts
    {
        return Err(DomainError::InvalidState(
            "model attempts exhausted fact has invalid attempt bounds".into(),
        ));
    }
    validate_non_negative_timestamp(
        "model attempts exhausted finished_at_ms",
        fact.finished_at_ms,
    )
}

fn validate_retry_schedule(schedule: &ModelRetrySchedule) -> Result<(), DomainError> {
    validate_identifier("activation_id", &schedule.activation_id)?;
    validate_identifier("round_id", &schedule.round_id)?;
    validate_identifier("request_id", &schedule.request_id)?;
    validate_identifier("failed_attempt_id", &schedule.failed_attempt_id)?;
    validate_identifier("retry error class", &schedule.error_class)?;
    if schedule.failed_attempt_number == 0
        || schedule.next_attempt_number != schedule.failed_attempt_number.saturating_add(1)
        || schedule.next_attempt_number > schedule.maximum_attempts
        || schedule.maximum_attempts == 0
        || schedule.maximum_attempts > MAX_MODEL_ATTEMPTS_PER_STEP
    {
        return Err(DomainError::InvalidState(
            "model retry schedule has invalid attempt bounds".into(),
        ));
    }
    validate_non_negative_timestamp("retry not_before_ms", schedule.not_before_ms)
}

fn validate_active_activation(activation: &ActiveActivation) -> Result<(), DomainError> {
    validate_identifier("activation_id", &activation.activation_id)?;
    activation.selection.validate()?;
    validate_non_negative_timestamp("activation started_at_ms", activation.started_at_ms)?;
    Ok(())
}

fn validate_active_model_round(round: &ActiveModelRound) -> Result<(), DomainError> {
    validate_identifier("activation_id", &round.activation_id)?;
    validate_identifier("round_id", &round.round_id)?;
    validate_non_negative_timestamp("model round started_at_ms", round.started_at_ms)?;
    if let Some(request) = &round.request {
        validate_identifier("request_id", &request.request_id)?;
        if request.activation_id != round.activation_id || request.round_id != round.round_id {
            return Err(DomainError::InvalidState(
                "declared model request belongs to another round".into(),
            ));
        }
        validate_model_fingerprint("request_fingerprint", &request.request_fingerprint)?;
        validate_model_fingerprint("prompt_fingerprint", &request.prompt_fingerprint)?;
        validate_model_fingerprint("tool_schema_fingerprint", &request.tool_schema_fingerprint)?;
        if request.maximum_attempts == 0 || request.maximum_attempts > MAX_MODEL_ATTEMPTS_PER_STEP {
            return Err(DomainError::InvalidState(
                "declared model request has invalid bounds".into(),
            ));
        }
        if let Some(attempt) = &round.attempt {
            if attempt.activation_id != round.activation_id
                || attempt.round_id != round.round_id
                || attempt.request_id != request.request_id
            {
                return Err(DomainError::InvalidState(
                    "model attempt belongs to another request".into(),
                ));
            }
            validate_identifier("attempt_id", &attempt.attempt_id)?;
            if attempt.attempt_number == 0 || attempt.attempt_number > request.maximum_attempts {
                return Err(DomainError::InvalidState(
                    "model attempt number is outside declared request bounds".into(),
                ));
            }
            validate_non_negative_timestamp("model attempt started_at_ms", attempt.started_at_ms)?;
            match (&attempt.outcome, &attempt.failure) {
                (ModelAttemptOutcome::Failed, Some(failure)) => {
                    validate_identifier("model attempt error class", &failure.error_class)?;
                }
                (ModelAttemptOutcome::Failed, None) => {
                    return Err(DomainError::InvalidState(
                        "failed model attempt has no failure cause".into(),
                    ));
                }
                (_, Some(_)) => {
                    return Err(DomainError::InvalidState(
                        "non-failed model attempt has a failure cause".into(),
                    ));
                }
                (_, None) => {}
            }
        }
    } else if round.attempt.is_some() || round.retry.is_some() {
        return Err(DomainError::InvalidState(
            "model attempt/retry requires a declared request".into(),
        ));
    }
    if let Some(schedule) = &round.retry {
        validate_retry_schedule(schedule)?;
        let request = round.request.as_ref().expect("checked above");
        if schedule.activation_id != round.activation_id
            || schedule.round_id != round.round_id
            || schedule.request_id != request.request_id
        {
            return Err(DomainError::InvalidState(
                "retry schedule belongs to another request".into(),
            ));
        }
    }
    Ok(())
}

fn current_model_attempt_mut<'a>(
    state: &'a mut SessionState,
    activation_id: &str,
    round_id: &str,
    request_id: &str,
    attempt_id: &str,
    attempt_number: u32,
) -> Result<&'a mut ModelAttemptRecord, DomainError> {
    let round = state
        .active_model_round
        .as_mut()
        .ok_or_else(|| DomainError::InvalidState("model attempt has no active round".into()))?;
    let request = round
        .request
        .as_ref()
        .ok_or_else(|| DomainError::InvalidState("model attempt has no declared request".into()))?;
    if request.activation_id != activation_id
        || request.round_id != round_id
        || request.request_id != request_id
        || round.activation_id != activation_id
        || round.round_id != round_id
    {
        return Err(DomainError::InvalidState(
            "model attempt belongs to another request".into(),
        ));
    }
    let attempt = round
        .attempt
        .as_mut()
        .ok_or_else(|| DomainError::InvalidState("model attempt has no started attempt".into()))?;
    if attempt.attempt_id != attempt_id || attempt.attempt_number != attempt_number {
        return Err(DomainError::InvalidState(
            "model attempt identity does not match".into(),
        ));
    }
    Ok(attempt)
}

fn validate_identifier(field: &'static str, value: &str) -> Result<(), DomainError> {
    require_text(field, value)?;
    if value.chars().any(char::is_control) {
        return Err(DomainError::InvalidState(format!(
            "{field} contains a control character"
        )));
    }
    Ok(())
}

fn validate_bounded_text(field: &'static str, value: &str) -> Result<(), DomainError> {
    require_text(field, value)
}

fn require_text(field: &'static str, value: &str) -> Result<(), DomainError> {
    if value.is_empty() {
        Err(DomainError::EmptyField { field })
    } else {
        Ok(())
    }
}

fn validate_workspace(workspace: &str) -> Result<(), DomainError> {
    require_text("workspace", workspace)?;
    if !Path::new(workspace).is_absolute() {
        return Err(DomainError::InvalidState(
            "workspace must be an absolute path".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_context_clones_share_the_raw_output_items() {
        let context = ProviderContext {
            profile_id: "profile".to_owned(),
            provider: "openai".to_owned(),
            model: "model".to_owned(),
            api: "openai-codex-responses".to_owned(),
            output_items: std::sync::Arc::new(vec![serde_json::json!({
                "type": "reasoning",
                "encrypted_content": "ciphertext",
            })]),
        };
        let cloned = context.clone();

        assert!(std::sync::Arc::ptr_eq(
            &context.output_items,
            &cloned.output_items,
        ));
    }

    #[test]
    fn every_created_object_uses_its_event_ulid() {
        let selection = SessionSelection {
            profile_id: "profile".to_owned(),
            model: "model".to_owned(),
            thinking: "max".to_owned(),
        };
        let drafts = EventDraft::batch(10, |ids| {
            vec![
                SessionEvent::SessionCreated {
                    schema_version: SESSION_CREATED_SCHEMA_VERSION,
                    session_id: ids[0].clone(),
                    created_at_ms: 1,
                    selection: selection.clone(),
                    system_prompt: None,
                    workspace: "/workspace".to_owned(),
                },
                SessionEvent::MailboxMessageAppended {
                    message: MailboxMessage {
                        message_id: ids[1].clone(),
                        mailbox_seq: 1,
                        content: Arc::from("input"),
                        received_at_ms: 2,
                    },
                },
                SessionEvent::MessageAppended {
                    message: TranscriptMessage {
                        message_id: ids[2].clone(),
                        role: TranscriptRole::Assistant,
                        content: Arc::from("result"),
                        is_error: false,
                        tool_call_id: None,
                        tool_calls: Vec::new(),
                        provider_context: None,
                        source_mailbox_seq: None,
                    },
                    wake_wait: false,
                },
                SessionEvent::ActivationStarted {
                    activation_id: ids[3].clone(),
                    selection: selection.clone(),
                    started_at_ms: 3,
                },
                SessionEvent::ModelRoundStarted {
                    activation_id: ids[3].clone(),
                    round_id: ids[4].clone(),
                    purpose: ModelRequestPurpose::Conversation,
                    mailbox_through_seq: 1,
                    started_at_ms: 4,
                },
                SessionEvent::ContextHandoffPlanned {
                    plan: ContextHandoffPlan {
                        plan_id: ids[5].clone(),
                        activation_id: ids[3].clone(),
                        previous_handoff_id: None,
                        next_generation: 2,
                        covered_through_message_id: ids[2].clone(),
                        max_output_tokens: 1,
                        selection: selection.clone(),
                    },
                },
                SessionEvent::ContextHandoffCreated {
                    handoff: ContextHandoffDocument {
                        handoff_id: ids[6].clone(),
                        plan_id: ids[5].clone(),
                        previous_handoff_id: None,
                        next_generation: 2,
                        covered_through_message_id: ids[2].clone(),
                        document: "handoff".to_owned(),
                        document_tokens: Some(1),
                        selection: selection.clone(),
                    },
                },
                SessionEvent::ModelRequestDeclared {
                    activation_id: ids[3].clone(),
                    round_id: ids[4].clone(),
                    request_id: ids[7].clone(),
                    request_fingerprint: "request".to_owned(),
                    prompt_fingerprint: "prompt".to_owned(),
                    tool_schema_fingerprint: "tools".to_owned(),
                    maximum_attempts: 1,
                },
                SessionEvent::ModelAttemptStarted {
                    activation_id: ids[3].clone(),
                    round_id: ids[4].clone(),
                    request_id: ids[7].clone(),
                    attempt_id: ids[8].clone(),
                    attempt_number: 1,
                    started_at_ms: 5,
                },
                SessionEvent::WaitSet {
                    wait: ActiveWait {
                        wait_id: ids[9].clone(),
                        reason: "wait".to_owned(),
                        timeout_seconds: 1,
                        deadline_ms: 6,
                        source: WaitSource::WaitFor,
                        tool_call_ids: vec!["call".to_owned()],
                    },
                },
            ]
        });

        for draft in drafts {
            assert_eq!(
                draft.event.created_object_id(),
                Some(draft.event_id.as_str())
            );
            let parsed = ulid::Ulid::from_string(&draft.event_id).unwrap();
            assert_eq!(parsed.to_string(), draft.event_id);
        }
    }

    fn created_state() -> SessionState {
        SessionState::new("session")
            .apply_event(&SessionEvent::SessionCreated {
                schema_version: SESSION_CREATED_SCHEMA_VERSION,
                session_id: "session".to_owned(),
                created_at_ms: 1,
                selection: SessionSelection {
                    profile_id: "profile".to_owned(),
                    model: "model".to_owned(),
                    thinking: "off".to_owned(),
                },
                system_prompt: None,
                workspace: "/workspace".to_owned(),
            })
            .unwrap()
    }

    #[test]
    fn durable_state_has_no_generic_payload_or_collection_admission_caps() {
        let mut mailbox_state = created_state();
        mailbox_state.mailbox = (1..=4_096)
            .map(|sequence| MailboxMessage {
                message_id: format!("mailbox-{sequence}"),
                mailbox_seq: sequence,
                content: Arc::from("x"),
                received_at_ms: 1,
            })
            .collect();
        assert!(mailbox_state
            .apply_event(&SessionEvent::MailboxMessageAppended {
                message: MailboxMessage {
                    message_id: "mailbox-4097".to_owned(),
                    mailbox_seq: 4_097,
                    content: Arc::from("x"),
                    received_at_ms: 1,
                },
            })
            .is_ok());

        let mut transcript_state = created_state();
        transcript_state.transcript = (0..8_192)
            .map(|index| TranscriptMessage {
                message_id: format!("message-{index}"),
                role: TranscriptRole::Assistant,
                content: Arc::from("x"),
                is_error: false,
                tool_call_id: None,
                tool_calls: Vec::new(),
                provider_context: None,
                source_mailbox_seq: None,
            })
            .collect();
        assert!(transcript_state
            .apply_event(&SessionEvent::MessageAppended {
                message: TranscriptMessage {
                    message_id: "message-8192".to_owned(),
                    role: TranscriptRole::Assistant,
                    content: Arc::from("x"),
                    is_error: false,
                    tool_call_id: None,
                    tool_calls: Vec::new(),
                    provider_context: None,
                    source_mailbox_seq: None,
                },
                wake_wait: false,
            })
            .is_ok());

        let message = TranscriptMessage {
            message_id: "many-provider-items".to_owned(),
            role: TranscriptRole::Assistant,
            content: Arc::from("x"),
            is_error: false,
            tool_call_id: None,
            tool_calls: (0..129)
                .map(|index| ToolCall {
                    tool_call_id: format!("call-{index}"),
                    tool_name: "tool".to_owned(),
                    arguments: serde_json::json!({}),
                })
                .collect(),
            provider_context: Some(ProviderContext {
                profile_id: "profile".to_owned(),
                provider: "provider".to_owned(),
                model: "model".to_owned(),
                api: "responses".to_owned(),
                output_items: Arc::new(
                    (0..129)
                        .map(|index| {
                            serde_json::json!({
                                "id": format!("item-{index}"),
                                "type": "reasoning",
                                "encrypted_content": format!("encrypted-{index}"),
                            })
                        })
                        .collect(),
                ),
            }),
            source_mailbox_seq: None,
        };
        assert!(validate_message(&message).is_ok());
        assert!(OpaqueContinuation::new("provider", 1, "kind", vec![0; 2 * 1024 * 1024]).is_ok());
        assert!(validate_bounded_text("provider error", &"x".repeat(32 * 1024)).is_ok());
    }
}
