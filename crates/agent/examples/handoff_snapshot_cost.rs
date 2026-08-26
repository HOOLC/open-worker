use std::{
    cmp::Ordering,
    collections::VecDeque,
    env,
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use ulid::Ulid;
use zork_agent::session::state::{
    ActivationOutcome, ActiveActivation, ActiveModelRound, ContextHandoffDocument,
    ContextHandoffPlan, MailboxMessage, ModelAttemptOutcome, ModelAttemptRecord, ModelRequestFact,
    ModelRequestPurpose, ProviderMessage, SessionSelection, ToolCall, TranscriptMessage,
    TranscriptRole, EVENT_SCHEMA_VERSION, SESSION_CREATED_SCHEMA_VERSION,
};
use zork_agent::SessionEvent;

const MIB: u64 = 1024 * 1024;
const IO_BLOCK_BYTES: usize = 64 * 1024;
const SNAPSHOT_STATE_SCHEMA_VERSION: u32 = 1;
const SNAPSHOT_REDUCER_SCHEMA_VERSION: u32 = 1;
const EXPECTED_FILE: &str = ".handoff-snapshot-expected.json";
const RESULT_FILE: &str = ".handoff-snapshot-result.json";
const TOKEN_ESTIMATED_BYTES_PER_TOKEN: u64 = 4;
const TOKEN_BASE_TOKENS: u64 = 256;
const TOKEN_MESSAGE_FRAMING_TOKENS: u64 = 64;
const TOKEN_TOOL_FRAMING_TOKENS: u64 = 128;
const TOKEN_ACCOUNTING_SCALE_MILLIONTHS: u64 = 1_000_000;
const TOKEN_SCENARIO_MAX_OUTPUT_TOKENS: u32 = 8_192;
const TOKEN_SCENARIO_BUFFER_TOKENS: u64 = 32_000;
const TOKEN_SCENARIO_SUFFIX_TARGET_TOKENS: u64 = 20_000;
const TOKEN_SCENARIO_USER_MESSAGE_BYTES: usize = 3 * 1024;
const TOKEN_SCENARIO_ASSISTANT_MESSAGE_BYTES: usize = 6 * 1024;
const TOKEN_SCENARIO_HANDOFF_DOCUMENT_BYTES: usize = 16_128;
const TOKEN_SCENARIO_WINDOWS: [u64; 6] =
    [100_000, 200_000, 500_000, 1_000_000, 2_000_000, 10_000_000];
const BENCHMARK_SYSTEM_PROMPT: &str =
    "You are a coding agent continuing one durable benchmark session.";
const CONTEXT_HANDOFF_INSTRUCTION: &str = r#"The runtime now requires a context handoff. Call context_handoff exactly once and put the complete handoff document in its document argument. Do not call any other tool in this response.

This is semantic compression, not a transcript copy. Preserve what is required to continue the same work correctly: the user's long-term goal; the current goal; the actual current state; confirmed decisions and boundaries; the most important observations and inferences; next actions and user-observable acceptance conditions. Do not copy large original passages. For important evidence, state the natural way to retrieve the original. Do not invent citation IDs, reference tables, message indexes, or another schema."#;

#[derive(Clone, Copy)]
struct Workload {
    main_turns: u64,
    suffix_turns: u64,
    turns_per_batch: u64,
    user_message_bytes: usize,
    assistant_message_bytes: usize,
    handoff_document_bytes: usize,
    segment_target_bytes: u64,
}

const DEFAULT_WORKLOAD: Workload = Workload {
    main_turns: 3_500,
    suffix_turns: 119,
    turns_per_batch: 7,
    user_message_bytes: 2 * 1024,
    assistant_message_bytes: 4 * 1024,
    handoff_document_bytes: 16 * 1024,
    segment_target_bytes: 16 * MIB,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
struct TokenScenario {
    context_window_tokens: u64,
    max_output_tokens: u32,
    buffer_tokens: u64,
    normal_input_budget_tokens: u64,
    handoff_input_budget_tokens: u64,
    calibration_scale_millionths: u64,
    previous_input_tokens: u64,
    estimated_input_tokens: u64,
    overshoot_tokens: u64,
    last_turn_tokens: u64,
    provider_transcript_json_bytes: u64,
    provider_message_count: u64,
    latest_boundary_handoff_source_tokens: u64,
    selected_boundary_handoff_source_tokens: u64,
    selected_boundary_message_index: u64,
    tail_message_count: u64,
    next_generation_input_tokens: u64,
    next_generation_transcript_json_bytes: u64,
    next_generation_after_suffix_input_tokens: u64,
    next_generation_after_suffix_transcript_json_bytes: u64,
    suffix_estimated_tokens: u64,
}

struct CalibratedContextWorkload {
    workload: Workload,
    scenario: TokenScenario,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct EventRef {
    event_id: String,
    stream_version: u64,
}

impl EventRef {
    fn from_record(record: &DomainRecord) -> Self {
        Self {
            event_id: record.event_id.clone(),
            stream_version: record.stream_version,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct MailboxRef {
    event: EventRef,
    message_id: String,
    mailbox_seq: u64,
    received_at_ms: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct MessageRef {
    event: EventRef,
    message_id: String,
    role: TranscriptRole,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_mailbox_seq: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct HandoffRef {
    event: EventRef,
    plan_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous_handoff_id: Option<String>,
    next_generation: u64,
    boundary: MessageRef,
    selection: SessionSelection,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
struct CompactProjection {
    created_at_ms: Option<i64>,
    selection: SessionSelection,
    consumed_through_mailbox_seq: u64,
    #[serde(default, skip_serializing_if = "VecDeque::is_empty")]
    pending_mailbox: VecDeque<MailboxRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    history_head: Option<MessageRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    current_input: Option<MessageRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    active_activation: Option<ActiveActivation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    active_model_round: Option<ActiveModelRound>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pending_context_handoff: Option<ContextHandoffPlan>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    latest_handoff: Option<HandoffRef>,
}

#[derive(Clone, Debug, PartialEq)]
struct ProjectionAt {
    stream_id: String,
    stream_version: u64,
    last_domain_event_id: String,
    state: CompactProjection,
}

impl ProjectionAt {
    fn empty(stream_id: impl Into<String>) -> Self {
        Self {
            stream_id: stream_id.into(),
            stream_version: 0,
            last_domain_event_id: String::new(),
            state: CompactProjection::default(),
        }
    }

    fn apply(&mut self, record: &DomainRecord) -> Result<()> {
        if record.stream_id != self.stream_id
            || record.event_schema_version != EVENT_SCHEMA_VERSION
            || record.stream_version != self.stream_version + 1
            || (!self.last_domain_event_id.is_empty()
                && record.event_id <= self.last_domain_event_id)
        {
            bail!("domain record position is invalid");
        }
        record.event.validate()?;
        if created_object_id(&record.event).is_some_and(|object_id| object_id != record.event_id) {
            bail!("event-created object does not use its Event ULID");
        }
        let event_ref = EventRef::from_record(record);
        match &record.event {
            SessionEvent::SessionCreated {
                session_id,
                created_at_ms,
                selection,
                ..
            } => {
                if self.stream_version != 0 || session_id != &self.stream_id {
                    bail!("invalid session creation");
                }
                self.state.created_at_ms = Some(*created_at_ms);
                self.state.selection = selection.clone();
            }
            SessionEvent::SelectionChanged { selection } => {
                if self.state.active_activation.is_some() || self.state.active_model_round.is_some()
                {
                    bail!("selection changed during an active effect");
                }
                self.state.selection = selection.clone();
            }
            SessionEvent::MailboxMessageAppended { message } => {
                let expected = self
                    .state
                    .consumed_through_mailbox_seq
                    .checked_add(self.state.pending_mailbox.len() as u64 + 1)
                    .context("mailbox sequence overflow")?;
                if message.mailbox_seq != expected {
                    bail!("mailbox order is invalid");
                }
                self.state.pending_mailbox.push_back(MailboxRef {
                    event: event_ref,
                    message_id: message.message_id.clone(),
                    mailbox_seq: message.mailbox_seq,
                    received_at_ms: message.received_at_ms,
                });
            }
            SessionEvent::MailboxDrained {
                through_mailbox_seq,
            } => {
                let available = self
                    .state
                    .pending_mailbox
                    .back()
                    .map(|message| message.mailbox_seq)
                    .context("mailbox drain has no pending message")?;
                if *through_mailbox_seq > available {
                    bail!("mailbox drain exceeds pending messages");
                }
                let mut last = None;
                while self
                    .state
                    .pending_mailbox
                    .front()
                    .is_some_and(|message| message.mailbox_seq <= *through_mailbox_seq)
                {
                    last = self.state.pending_mailbox.pop_front();
                }
                let last = last.context("mailbox drain did not consume a message")?;
                let message = MessageRef {
                    event: last.event,
                    message_id: last.message_id,
                    role: TranscriptRole::User,
                    source_mailbox_seq: Some(last.mailbox_seq),
                };
                self.state.history_head = Some(message.clone());
                self.state.current_input = Some(message);
                self.state.consumed_through_mailbox_seq = *through_mailbox_seq;
            }
            SessionEvent::MessageAppended { message, .. } => {
                let message_ref = MessageRef {
                    event: event_ref,
                    message_id: message.message_id.clone(),
                    role: message.role.clone(),
                    source_mailbox_seq: message.source_mailbox_seq,
                };
                self.state.history_head = Some(message_ref);
                if message.role == TranscriptRole::Assistant {
                    self.state.current_input = None;
                }
            }
            SessionEvent::ActivationStarted {
                activation_id,
                selection,
                started_at_ms,
            } => {
                if self.state.active_activation.is_some() || selection != &self.state.selection {
                    bail!("invalid activation start");
                }
                self.state.active_activation = Some(ActiveActivation {
                    activation_id: activation_id.clone(),
                    selection: selection.clone(),
                    started_at_ms: *started_at_ms,
                });
            }
            SessionEvent::ModelRoundStarted {
                activation_id,
                round_id,
                purpose,
                mailbox_through_seq,
                started_at_ms,
            } => {
                if self
                    .state
                    .active_activation
                    .as_ref()
                    .is_none_or(|activation| activation.activation_id != *activation_id)
                {
                    bail!("model round has no matching activation");
                }
                self.state.active_model_round = Some(ActiveModelRound {
                    activation_id: activation_id.clone(),
                    round_id: round_id.clone(),
                    purpose: purpose.clone(),
                    mailbox_through_seq: *mailbox_through_seq,
                    started_at_ms: *started_at_ms,
                    request: None,
                    attempt: None,
                    retry: None,
                });
            }
            SessionEvent::ContextHandoffPlanned { plan } => {
                if self.state.pending_context_handoff.is_some() {
                    bail!("context handoff already planned");
                }
                self.state.pending_context_handoff = Some(plan.clone());
            }
            SessionEvent::ModelRequestDeclared {
                activation_id,
                round_id,
                request_id,
                request_fingerprint,
                prompt_fingerprint,
                tool_schema_fingerprint,
                maximum_attempts,
            } => {
                let round = self
                    .state
                    .active_model_round
                    .as_mut()
                    .context("model request has no round")?;
                if round.activation_id != *activation_id
                    || round.round_id != *round_id
                    || round.request.is_some()
                {
                    bail!("model request does not match the round");
                }
                round.request = Some(ModelRequestFact {
                    activation_id: activation_id.clone(),
                    round_id: round_id.clone(),
                    request_id: request_id.clone(),
                    request_fingerprint: request_fingerprint.clone(),
                    prompt_fingerprint: prompt_fingerprint.clone(),
                    tool_schema_fingerprint: tool_schema_fingerprint.clone(),
                    maximum_attempts: *maximum_attempts,
                });
            }
            SessionEvent::ModelAttemptStarted {
                activation_id,
                round_id,
                request_id,
                attempt_id,
                attempt_number,
                started_at_ms,
            } => {
                let round = self
                    .state
                    .active_model_round
                    .as_mut()
                    .context("model attempt has no round")?;
                let request = round
                    .request
                    .as_ref()
                    .context("model attempt has no request")?;
                if request.activation_id != *activation_id
                    || request.round_id != *round_id
                    || request.request_id != *request_id
                    || round.attempt.is_some()
                {
                    bail!("model attempt does not match the request");
                }
                round.attempt = Some(ModelAttemptRecord {
                    activation_id: activation_id.clone(),
                    round_id: round_id.clone(),
                    request_id: request_id.clone(),
                    attempt_id: attempt_id.clone(),
                    attempt_number: *attempt_number,
                    started_at_ms: *started_at_ms,
                    outcome: ModelAttemptOutcome::Running,
                    failure: None,
                });
            }
            SessionEvent::ModelRequestCompleted {
                activation_id,
                round_id,
                request_id,
                attempt_id,
                ..
            } => {
                let round = self
                    .state
                    .active_model_round
                    .as_mut()
                    .context("model completion has no round")?;
                let attempt = round
                    .attempt
                    .as_mut()
                    .context("model completion has no attempt")?;
                if attempt.activation_id != *activation_id
                    || attempt.round_id != *round_id
                    || attempt.request_id != *request_id
                    || attempt.attempt_id != *attempt_id
                    || attempt.outcome != ModelAttemptOutcome::Running
                {
                    bail!("model completion does not match the running attempt");
                }
                attempt.outcome = ModelAttemptOutcome::Completed;
            }
            SessionEvent::ContextHandoffCreated { handoff } => {
                let plan = self
                    .state
                    .pending_context_handoff
                    .as_ref()
                    .context("handoff document has no plan")?;
                if handoff.plan_id != plan.plan_id
                    || handoff.covered_through_message_id != plan.covered_through_message_id
                {
                    bail!("handoff document does not match its plan");
                }
                let boundary = self
                    .state
                    .history_head
                    .clone()
                    .context("handoff has no message boundary")?;
                if boundary.message_id != handoff.covered_through_message_id {
                    bail!("handoff boundary is not the history head");
                }
                self.state.latest_handoff = Some(HandoffRef {
                    event: event_ref,
                    plan_id: handoff.plan_id.clone(),
                    previous_handoff_id: handoff.previous_handoff_id.clone(),
                    next_generation: handoff.next_generation,
                    boundary,
                    selection: handoff.selection.clone(),
                });
                self.state.pending_context_handoff = None;
            }
            SessionEvent::ActivationFinished { activation_id, .. } => {
                if self
                    .state
                    .active_activation
                    .as_ref()
                    .is_none_or(|activation| activation.activation_id != *activation_id)
                {
                    bail!("activation finish does not match the active activation");
                }
                self.state.active_activation = None;
                self.state.active_model_round = None;
                self.state.pending_context_handoff = None;
            }
            other => bail!("benchmark projection does not support {}", other.kind()),
        }
        self.stream_version = record.stream_version;
        self.last_domain_event_id = record.event_id.clone();
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct DomainRecord {
    event_id: String,
    stream_id: String,
    stream_version: u64,
    event_schema_version: u32,
    batch_index: u32,
    batch_size: u32,
    event: SessionEvent,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum StoredRecord {
    Domain {
        event_id: String,
        stream_id: String,
        stream_version: u64,
        event_schema_version: u32,
        batch_index: u32,
        batch_size: u32,
        event: Box<SessionEvent>,
    },
    Snapshot {
        event_id: String,
        stream_id: String,
        batch_index: u32,
        batch_size: u32,
        through_event_id: String,
        through_stream_version: u64,
        state_schema_version: u32,
        reducer_schema_version: u32,
        state: Box<CompactProjection>,
    },
}

impl StoredRecord {
    fn event_id(&self) -> &str {
        match self {
            Self::Domain { event_id, .. } | Self::Snapshot { event_id, .. } => event_id,
        }
    }

    fn domain_record(&self) -> Option<DomainRecord> {
        let Self::Domain {
            event_id,
            stream_id,
            stream_version,
            event_schema_version,
            batch_index,
            batch_size,
            event,
        } = self
        else {
            return None;
        };
        Some(DomainRecord {
            event_id: event_id.clone(),
            stream_id: stream_id.clone(),
            stream_version: *stream_version,
            event_schema_version: *event_schema_version,
            batch_index: *batch_index,
            batch_size: *batch_size,
            event: event.as_ref().clone(),
        })
    }
}

struct DomainBatch {
    records: Vec<DomainRecord>,
    bytes: Vec<u8>,
}

struct OrderedIds {
    next: u128,
}

impl OrderedIds {
    fn new() -> Self {
        Self { next: 1 }
    }

    fn next(&mut self) -> String {
        let value = Ulid::from_parts(1_700_000_000_000, self.next).to_string();
        self.next += 1;
        value
    }

    fn peek(&self) -> String {
        Ulid::from_parts(1_700_000_000_000, self.next).to_string()
    }
}

struct SegmentWriter {
    segments_dir: PathBuf,
    current: Option<PathBuf>,
    current_bytes: u64,
    sealed: Vec<PathBuf>,
    domain_jsonl_bytes: u64,
    snapshot_jsonl_bytes: u64,
    snapshot_count: u64,
    durable_append_count: u64,
}

impl SegmentWriter {
    fn create(root: &Path) -> Result<Self> {
        let segments_dir = root.join("segments");
        fs::create_dir_all(&segments_dir)?;
        sync_directory(&segments_dir)?;
        Ok(Self {
            segments_dir,
            current: None,
            current_bytes: 0,
            sealed: Vec::new(),
            domain_jsonl_bytes: 0,
            snapshot_jsonl_bytes: 0,
            snapshot_count: 0,
            durable_append_count: 0,
        })
    }

    fn current_bytes(&self) -> u64 {
        self.current_bytes
    }

    fn roll_before_next_batch(&mut self) -> Result<()> {
        let current = self.current.take().context("cannot roll an empty stream")?;
        self.sealed.push(current);
        self.current_bytes = 0;
        Ok(())
    }

    fn append_domain(&mut self, batch: &DomainBatch) -> Result<PathBuf> {
        let first = batch.records.first().context("domain batch is empty")?;
        let path = self.append_bytes(&first.event_id, &batch.bytes)?;
        let len = u64::try_from(batch.bytes.len())?;
        self.domain_jsonl_bytes = self
            .domain_jsonl_bytes
            .checked_add(len)
            .context("domain byte count overflow")?;
        Ok(path)
    }

    fn append_handoff_with_snapshot(
        &mut self,
        handoff: &DomainBatch,
        snapshot_bytes: &[u8],
    ) -> Result<PathBuf> {
        let first = handoff.records.first().context("handoff batch is empty")?;
        let mut bytes = Vec::with_capacity(handoff.bytes.len() + snapshot_bytes.len());
        bytes.extend_from_slice(&handoff.bytes);
        bytes.extend_from_slice(snapshot_bytes);
        let path = self.append_bytes(&first.event_id, &bytes)?;
        self.domain_jsonl_bytes = self
            .domain_jsonl_bytes
            .checked_add(u64::try_from(handoff.bytes.len())?)
            .context("domain byte count overflow")?;
        self.snapshot_jsonl_bytes = self
            .snapshot_jsonl_bytes
            .checked_add(u64::try_from(snapshot_bytes.len())?)
            .context("snapshot byte count overflow")?;
        self.snapshot_count += 1;
        Ok(path)
    }

    fn append_bytes(&mut self, first_event_id: &str, bytes: &[u8]) -> Result<PathBuf> {
        let path = match &self.current {
            Some(path) => path.clone(),
            None => {
                let path = self.segments_dir.join(format!("{first_event_id}.jsonl"));
                let mut options = OpenOptions::new();
                options.create_new(true).write(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                let mut file = options.open(&path)?;
                file.write_all(bytes)?;
                file.sync_all()?;
                sync_directory(&self.segments_dir)?;
                self.durable_append_count += 1;
                self.current = Some(path.clone());
                self.current_bytes = u64::try_from(bytes.len())?;
                return Ok(path);
            }
        };
        let mut file = OpenOptions::new().append(true).open(&path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        self.durable_append_count += 1;
        self.current_bytes = self
            .current_bytes
            .checked_add(u64::try_from(bytes.len())?)
            .context("current segment size overflow")?;
        Ok(path)
    }

    fn finish(self) -> Result<StorageStats> {
        let mut compression_us = Vec::new();
        for path in &self.sealed {
            let started = Instant::now();
            compress_segment(path)?;
            compression_us.push(duration_us(started.elapsed())?);
        }
        let current = self.current.context("stream has no current segment")?;
        let current_segment_bytes = fs::metadata(&current)?.len();
        let mut physical_segment_bytes = current_segment_bytes;
        for path in &self.sealed {
            physical_segment_bytes = physical_segment_bytes
                .checked_add(fs::metadata(compressed_path(path))?.len())
                .context("physical size overflow")?;
        }
        Ok(StorageStats {
            domain_jsonl_bytes: self.domain_jsonl_bytes,
            snapshot_jsonl_bytes: self.snapshot_jsonl_bytes,
            logical_jsonl_bytes: self
                .domain_jsonl_bytes
                .checked_add(self.snapshot_jsonl_bytes)
                .context("logical size overflow")?,
            physical_segment_bytes,
            segment_count: u64::try_from(self.sealed.len() + 1)?,
            compressed_segment_count: u64::try_from(self.sealed.len())?,
            current_segment_bytes,
            snapshot_count: self.snapshot_count,
            durable_append_count: self.durable_append_count,
            compression_latency: latency_summary(compression_us),
        })
    }
}

#[derive(Debug, Serialize)]
struct LatencySummary {
    samples: usize,
    total_us: u64,
    min_us: u64,
    median_us: u64,
    p95_us: u64,
    max_us: u64,
}

#[derive(Debug, Serialize)]
struct StorageStats {
    domain_jsonl_bytes: u64,
    snapshot_jsonl_bytes: u64,
    logical_jsonl_bytes: u64,
    physical_segment_bytes: u64,
    segment_count: u64,
    compressed_segment_count: u64,
    current_segment_bytes: u64,
    snapshot_count: u64,
    durable_append_count: u64,
    compression_latency: LatencySummary,
}

#[derive(Debug, Serialize)]
struct GenerateResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    token_scenario: Option<TokenScenario>,
    handoffs: u64,
    main_turns: u64,
    suffix_turns: u64,
    handoff_document_bytes: usize,
    snapshot_state_json_bytes: u64,
    snapshot_record_jsonl_bytes: u64,
    snapshot_prepare_latency: LatencySummary,
    proposed_handoff_latency: LatencySummary,
    baseline_handoff_latency: LatencySummary,
    proposed: StorageStats,
    baseline: StorageStats,
    logical_amplification: f64,
    physical_amplification: f64,
    logical_bytes_per_handoff: f64,
    physical_bytes_per_handoff: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Expected {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    token_scenario: Option<TokenScenario>,
    handoffs: u64,
    stream_id: String,
    stream_version: u64,
    last_domain_event_id: String,
    state: CompactProjection,
    latest_handoff_id: String,
    latest_document_digest: String,
    latest_snapshot_event_id: String,
}

#[derive(Debug, Serialize)]
struct RecoveryResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    context_window_tokens: Option<u64>,
    handoffs: u64,
    mode: String,
    elapsed_us: u64,
    peak_rss_bytes: u64,
    physical_bytes_read: u64,
    logical_bytes_read: u64,
    domain_events_applied: u64,
    stream_version: u64,
}

#[derive(Debug, Serialize)]
struct LookupResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    context_window_tokens: Option<u64>,
    handoffs: u64,
    elapsed_us: u64,
    peak_rss_bytes: u64,
    physical_bytes_read: u64,
    logical_bytes_read: u64,
    segment_id: String,
    handoff_id: String,
    document_bytes: usize,
}

struct ReadResult {
    records: Vec<StoredRecord>,
    physical_bytes: u64,
    logical_bytes: u64,
}

struct Recovery {
    projection: ProjectionAt,
    physical_bytes_read: u64,
    logical_bytes_read: u64,
    domain_events_applied: u64,
}

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let Some(command) = args.next() else {
        bail!(
            "usage: handoff_snapshot_cost <generate ROOT HANDOFFS|calibrate-context CONTEXT_TOKENS|generate-context ROOT CONTEXT_TOKENS|rehydrate ROOT snapshot|baseline|lookup ROOT>"
        );
    };
    match command.as_str() {
        "generate" => {
            let root = required_path(args.next(), "ROOT")?;
            let handoffs = args
                .next()
                .context("missing HANDOFFS")?
                .parse::<u64>()
                .context("HANDOFFS must be an integer")?;
            require_no_extra(args)?;
            let result = generate(&root, handoffs, DEFAULT_WORKLOAD, None)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        "generate-context" => {
            let root = required_path(args.next(), "ROOT")?;
            let context_window_tokens = args
                .next()
                .context("missing CONTEXT_TOKENS")?
                .parse::<u64>()
                .context("CONTEXT_TOKENS must be an integer")?;
            require_no_extra(args)?;
            let calibrated = calibrate_context_workload(context_window_tokens)?;
            let result = generate(&root, 1, calibrated.workload, Some(calibrated.scenario))?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        "calibrate-context" => {
            let context_window_tokens = args
                .next()
                .context("missing CONTEXT_TOKENS")?
                .parse::<u64>()
                .context("CONTEXT_TOKENS must be an integer")?;
            require_no_extra(args)?;
            let calibrated = calibrate_context_workload(context_window_tokens)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "main_turns": calibrated.workload.main_turns,
                    "suffix_turns": calibrated.workload.suffix_turns,
                    "scenario": calibrated.scenario,
                }))?
            );
        }
        "rehydrate" => {
            let root = required_path(args.next(), "ROOT")?;
            let mode = args.next().context("missing recovery mode")?;
            require_no_extra(args)?;
            rehydrate_command(&root, &mode)?;
        }
        "lookup" => {
            let root = required_path(args.next(), "ROOT")?;
            require_no_extra(args)?;
            lookup_command(&root)?;
        }
        _ => bail!("unknown command: {command}"),
    }
    Ok(())
}

fn required_path(value: Option<String>, name: &str) -> Result<PathBuf> {
    value
        .map(PathBuf::from)
        .with_context(|| format!("missing {name}"))
}

fn require_no_extra(mut args: impl Iterator<Item = String>) -> Result<()> {
    if args.next().is_some() {
        bail!("unexpected argument");
    }
    Ok(())
}

struct TranscriptMeasure {
    json_bytes: u64,
    message_count: u64,
}

impl TranscriptMeasure {
    fn empty() -> Self {
        Self {
            json_bytes: 2,
            message_count: 0,
        }
    }

    fn append(&mut self, message: &ProviderMessage) -> Result<()> {
        let message_bytes = u64::try_from(serde_json::to_vec(message)?.len())?;
        self.json_bytes = self
            .json_bytes
            .saturating_add(u64::from(self.message_count > 0))
            .saturating_add(message_bytes);
        self.message_count = self.message_count.saturating_add(1);
        Ok(())
    }

    fn estimated_tokens(&self) -> u64 {
        local_token_estimate(self.json_bytes, self.message_count, 2, 0)
    }
}

fn local_token_estimate(
    transcript_json_bytes: u64,
    message_count: u64,
    tool_json_bytes: u64,
    tool_count: u64,
) -> u64 {
    TOKEN_BASE_TOKENS
        .saturating_add(
            transcript_json_bytes
                .saturating_add(tool_json_bytes)
                .div_ceil(TOKEN_ESTIMATED_BYTES_PER_TOKEN),
        )
        .saturating_add(TOKEN_MESSAGE_FRAMING_TOKENS.saturating_mul(message_count))
        .saturating_add(TOKEN_TOOL_FRAMING_TOKENS.saturating_mul(tool_count))
}

fn scaled_token_estimate(local: u64) -> u64 {
    local
        .saturating_mul(TOKEN_ACCOUNTING_SCALE_MILLIONTHS)
        .div_ceil(1_000_000)
}

fn estimated_text_tokens(bytes: usize) -> Result<u64> {
    Ok(TOKEN_MESSAGE_FRAMING_TOKENS
        .saturating_add(u64::try_from(bytes)?.div_ceil(TOKEN_ESTIMATED_BYTES_PER_TOKEN)))
}

fn measure_transcript(messages: &[ProviderMessage]) -> Result<TranscriptMeasure> {
    Ok(TranscriptMeasure {
        json_bytes: u64::try_from(serde_json::to_vec(messages)?.len())?,
        message_count: u64::try_from(messages.len())?,
    })
}

fn handoff_source_measure(
    transcript: &[ProviderMessage],
    boundary: usize,
) -> Result<TranscriptMeasure> {
    let source = transcript
        .get(..=boundary)
        .context("handoff boundary is outside the transcript")?;
    let mut projected = Vec::with_capacity(source.len() + 2);
    projected.push(benchmark_system_message());
    projected.extend(source.iter().cloned());
    projected.push(ProviderMessage {
        role: TranscriptRole::System,
        content: CONTEXT_HANDOFF_INSTRUCTION.into(),
        is_error: false,
        tool_call_id: None,
        tool_calls: Vec::new(),
        provider_context: None,
    });
    measure_transcript(&projected)
}

fn next_generation_context(
    transcript: &[ProviderMessage],
    boundary: usize,
) -> Result<Vec<ProviderMessage>> {
    transcript
        .get(..=boundary)
        .context("next-generation boundary is outside the transcript")?;
    let handoff_id = Ulid::from_parts(1_700_000_000_000, 1).to_string();
    let tool_call_id = format!("call_{handoff_id}");
    let mut context = vec![benchmark_system_message()];
    context.push(ProviderMessage {
        role: TranscriptRole::Assistant,
        content: "".into(),
        is_error: false,
        tool_call_id: None,
        tool_calls: vec![ToolCall {
            tool_call_id: tool_call_id.clone(),
            tool_name: "read_context_handoff".to_owned(),
            arguments: json!({}),
        }],
        provider_context: None,
    });
    context.push(ProviderMessage {
        role: TranscriptRole::Tool,
        content: deterministic_content(50_001, TOKEN_SCENARIO_HANDOFF_DOCUMENT_BYTES).into(),
        is_error: false,
        tool_call_id: Some(tool_call_id),
        tool_calls: Vec::new(),
        provider_context: None,
    });
    context.extend(transcript[boundary + 1..].iter().cloned());
    Ok(context)
}

fn calibrate_context_workload(context_window_tokens: u64) -> Result<CalibratedContextWorkload> {
    if !TOKEN_SCENARIO_WINDOWS.contains(&context_window_tokens) {
        bail!("CONTEXT_TOKENS must be one of 100000, 200000, 500000, 1000000, 2000000, 10000000");
    }
    let reserved_tokens = u64::from(TOKEN_SCENARIO_MAX_OUTPUT_TOKENS)
        .checked_add(TOKEN_SCENARIO_BUFFER_TOKENS)
        .context("token reserve overflow")?;
    let normal_input_budget_tokens = context_window_tokens
        .checked_sub(reserved_tokens)
        .context("context window does not leave a normal input budget")?;
    let handoff_input_budget_tokens = context_window_tokens
        .checked_sub(u64::from(TOKEN_SCENARIO_MAX_OUTPUT_TOKENS))
        .context("context window does not leave a handoff input budget")?;

    let workload_template = Workload {
        main_turns: 0,
        suffix_turns: 0,
        turns_per_batch: 7,
        user_message_bytes: TOKEN_SCENARIO_USER_MESSAGE_BYTES,
        assistant_message_bytes: TOKEN_SCENARIO_ASSISTANT_MESSAGE_BYTES,
        handoff_document_bytes: TOKEN_SCENARIO_HANDOFF_DOCUMENT_BYTES,
        segment_target_bytes: 16 * MIB,
    };
    let mut transcript = TranscriptMeasure::empty();
    transcript.append(&benchmark_system_message())?;
    let mut messages = Vec::new();
    let mut previous_input_tokens = scaled_token_estimate(transcript.estimated_tokens());
    let mut turn = 0_u64;
    let (main_turns, estimated_input_tokens, last_turn_tokens) = loop {
        turn = turn.saturating_add(1);
        let user = simulated_user_provider_message(turn, workload_template);
        let assistant = simulated_assistant_provider_message(turn, workload_template);
        transcript.append(&user)?;
        transcript.append(&assistant)?;
        messages.push(user);
        messages.push(assistant);
        let estimated = scaled_token_estimate(transcript.estimated_tokens());
        if estimated > normal_input_budget_tokens {
            break (turn, estimated, estimated - previous_input_tokens);
        }
        previous_input_tokens = estimated;
    };

    let latest_boundary = messages
        .len()
        .checked_sub(1)
        .context("calibrated transcript has no boundary")?;
    let latest_boundary_handoff_source_tokens = scaled_token_estimate(
        handoff_source_measure(&messages, latest_boundary)?.estimated_tokens(),
    );
    if latest_boundary_handoff_source_tokens > handoff_input_budget_tokens {
        bail!("latest complete handoff source exceeds the model input budget");
    }
    let selected_boundary = latest_boundary;
    let selected_boundary_handoff_source_tokens = latest_boundary_handoff_source_tokens;
    let mut next_generation = next_generation_context(&messages, selected_boundary)?;
    let next_generation_measure = measure_transcript(&next_generation)?;
    let next_generation_input_tokens =
        scaled_token_estimate(next_generation_measure.estimated_tokens());

    let mut suffix = TranscriptMeasure::empty();
    let empty_suffix_tokens = scaled_token_estimate(suffix.estimated_tokens());
    let (suffix_turns, suffix_estimated_tokens) = loop {
        let suffix_turn = suffix.message_count / 2 + 1;
        let turn = main_turns + suffix_turn;
        suffix.append(&simulated_user_provider_message(turn, workload_template))?;
        suffix.append(&simulated_assistant_provider_message(
            turn,
            workload_template,
        ))?;
        let estimated = scaled_token_estimate(suffix.estimated_tokens()) - empty_suffix_tokens;
        if estimated >= TOKEN_SCENARIO_SUFFIX_TARGET_TOKENS {
            break (suffix_turn, estimated);
        }
    };
    for suffix_turn in 1..=suffix_turns {
        let turn = main_turns + suffix_turn;
        next_generation.push(simulated_user_provider_message(turn, workload_template));
        next_generation.push(simulated_assistant_provider_message(
            turn,
            workload_template,
        ));
    }
    let next_generation_after_suffix_measure = measure_transcript(&next_generation)?;
    let next_generation_after_suffix_input_tokens =
        scaled_token_estimate(next_generation_after_suffix_measure.estimated_tokens());

    let workload = Workload {
        main_turns,
        suffix_turns,
        ..workload_template
    };
    Ok(CalibratedContextWorkload {
        workload,
        scenario: TokenScenario {
            context_window_tokens,
            max_output_tokens: TOKEN_SCENARIO_MAX_OUTPUT_TOKENS,
            buffer_tokens: TOKEN_SCENARIO_BUFFER_TOKENS,
            normal_input_budget_tokens,
            handoff_input_budget_tokens,
            calibration_scale_millionths: TOKEN_ACCOUNTING_SCALE_MILLIONTHS,
            previous_input_tokens,
            estimated_input_tokens,
            overshoot_tokens: estimated_input_tokens - normal_input_budget_tokens,
            last_turn_tokens,
            provider_transcript_json_bytes: transcript.json_bytes,
            provider_message_count: transcript.message_count,
            latest_boundary_handoff_source_tokens,
            selected_boundary_handoff_source_tokens,
            selected_boundary_message_index: u64::try_from(selected_boundary)?,
            tail_message_count: u64::try_from(latest_boundary - selected_boundary)?,
            next_generation_input_tokens,
            next_generation_transcript_json_bytes: next_generation_measure.json_bytes,
            next_generation_after_suffix_input_tokens,
            next_generation_after_suffix_transcript_json_bytes:
                next_generation_after_suffix_measure.json_bytes,
            suffix_estimated_tokens,
        },
    })
}

fn benchmark_system_message() -> ProviderMessage {
    ProviderMessage {
        role: TranscriptRole::System,
        content: BENCHMARK_SYSTEM_PROMPT.into(),
        is_error: false,
        tool_call_id: None,
        tool_calls: Vec::new(),
        provider_context: None,
    }
}

fn generate(
    root: &Path,
    handoffs: u64,
    workload: Workload,
    token_scenario: Option<TokenScenario>,
) -> Result<GenerateResult> {
    if !matches!(handoffs, 1 | 2 | 4 | 8 | 16) {
        bail!("HANDOFFS must be one of 1, 2, 4, 8, 16");
    }
    if root.exists() && fs::read_dir(root)?.next().transpose()?.is_some() {
        bail!("benchmark root must be empty: {}", root.display());
    }
    fs::create_dir_all(root)?;
    let proposed_root = root.join("snapshot");
    let baseline_root = root.join("baseline");
    let mut proposed = SegmentWriter::create(&proposed_root)?;
    let mut baseline = SegmentWriter::create(&baseline_root)?;
    let mut ids = OrderedIds::new();
    let stream_id = ids.peek();
    let selection = SessionSelection {
        profile_id: "benchmark".to_owned(),
        model: "benchmark".to_owned(),
        thinking: "off".to_owned(),
    };
    let mut projection = ProjectionAt::empty(stream_id.clone());
    let initial = build_domain_batch(
        &stream_id,
        projection.stream_version,
        1,
        1,
        &mut ids,
        |event_ids| {
            Ok(vec![SessionEvent::SessionCreated {
                schema_version: SESSION_CREATED_SCHEMA_VERSION,
                session_id: event_ids[0].clone(),
                created_at_ms: 1,
                selection: selection.clone(),
                system_prompt: None,
                workspace: "/workspace".to_owned(),
            }])
        },
    )?;
    append_same_domain(&mut proposed, &mut baseline, &initial)?;
    apply_batch(&mut projection, &initial)?;

    let mut snapshot_prepare_us = Vec::new();
    let mut proposed_handoff_us = Vec::new();
    let mut baseline_handoff_us = Vec::new();
    let mut latest_snapshot_state_json_bytes = 0;
    let mut latest_snapshot_record_jsonl_bytes = 0;
    let mut latest_snapshot_event_id = String::new();
    let mut latest_document_digest = String::new();
    let mut next_turn = 1_u64;

    for index in 0..handoffs {
        let generation_end = workload
            .main_turns
            .checked_mul(index + 1)
            .context("generation boundary overflow")?
            / handoffs;
        append_turn_range(
            &mut proposed,
            &mut baseline,
            &mut projection,
            &mut ids,
            &selection,
            next_turn,
            generation_end,
            workload,
        )?;
        next_turn = generation_end + 1;

        let handoff_number = index + 1;
        let max_output_tokens = if let Some(scenario) = token_scenario.as_ref() {
            if scenario.tail_message_count != 0 || projection.state.history_head.is_none() {
                bail!("calibrated handoff boundary is not the benchmark history head");
            }
            scenario.max_output_tokens
        } else {
            TOKEN_SCENARIO_MAX_OUTPUT_TOKENS
        };
        let (preparation, plan, identity) = handoff_preparation(
            &stream_id,
            &projection,
            &selection,
            handoff_number,
            max_output_tokens,
            &mut ids,
        )?;
        append_same_domain(&mut proposed, &mut baseline, &preparation)?;
        apply_batch(&mut projection, &preparation)?;

        let document = deterministic_content(
            50_000_u64.wrapping_add(handoff_number),
            workload.handoff_document_bytes,
        );
        let document_digest = sha256_hex(document.as_bytes());
        let document_tokens = Some(estimated_text_tokens(workload.handoff_document_bytes)?);
        let handoff_batch = build_domain_batch(
            &stream_id,
            projection.stream_version,
            2,
            3,
            &mut ids,
            |event_ids| {
                Ok(vec![
                    SessionEvent::ContextHandoffCreated {
                        handoff: ContextHandoffDocument {
                            handoff_id: event_ids[0].clone(),
                            plan_id: plan.plan_id.clone(),
                            previous_handoff_id: plan.previous_handoff_id.clone(),
                            next_generation: plan.next_generation,
                            covered_through_message_id: plan.covered_through_message_id.clone(),
                            document: document.clone(),
                            document_tokens,
                            selection: selection.clone(),
                        },
                    },
                    SessionEvent::ModelRequestCompleted {
                        activation_id: identity.activation_id.clone(),
                        round_id: identity.round_id.clone(),
                        request_id: identity.request_id.clone(),
                        attempt_id: identity.attempt_id.clone(),
                        usage: None,
                        provider_input: None,
                    },
                ])
            },
        )?;
        let mut after_handoff = projection.clone();
        apply_batch(&mut after_handoff, &handoff_batch)?;
        let snapshot_event_id = ids.next();

        let should_roll = proposed.current_bytes() > workload.segment_target_bytes;
        if should_roll {
            proposed.roll_before_next_batch()?;
            baseline.roll_before_next_batch()?;
        }

        let baseline_first = index % 2 == 0;
        let write_proposed = |writer: &mut SegmentWriter| -> Result<(u64, u64, u64, u64)> {
            let total_started = Instant::now();
            let snapshot_prepare_started = Instant::now();
            let snapshot = StoredRecord::Snapshot {
                event_id: snapshot_event_id.clone(),
                stream_id: stream_id.clone(),
                batch_index: 2,
                batch_size: 3,
                through_event_id: after_handoff.last_domain_event_id.clone(),
                through_stream_version: after_handoff.stream_version,
                state_schema_version: SNAPSHOT_STATE_SCHEMA_VERSION,
                reducer_schema_version: SNAPSHOT_REDUCER_SCHEMA_VERSION,
                state: Box::new(after_handoff.state.clone()),
            };
            let snapshot_bytes = serialize_records(&[snapshot])?;
            validate_compact_snapshot_json(&snapshot_bytes)?;
            let snapshot_prepare_us = duration_us(snapshot_prepare_started.elapsed())?;
            writer.append_handoff_with_snapshot(&handoff_batch, &snapshot_bytes)?;
            let total_us = duration_us(total_started.elapsed())?;
            let state_json_bytes = u64::try_from(serde_json::to_vec(&after_handoff.state)?.len())?;
            Ok((
                total_us,
                snapshot_prepare_us,
                state_json_bytes,
                u64::try_from(snapshot_bytes.len())?,
            ))
        };
        let write_baseline = |writer: &mut SegmentWriter| -> Result<u64> {
            let started = Instant::now();
            writer.append_domain(&handoff_batch)?;
            duration_us(started.elapsed())
        };
        let (
            proposed_us,
            snapshot_prepare_elapsed_us,
            snapshot_state_bytes,
            snapshot_record_bytes,
            baseline_us,
        ) = if baseline_first {
            let baseline_us = write_baseline(&mut baseline)?;
            let (
                proposed_us,
                snapshot_prepare_elapsed_us,
                snapshot_state_bytes,
                snapshot_record_bytes,
            ) = write_proposed(&mut proposed)?;
            (
                proposed_us,
                snapshot_prepare_elapsed_us,
                snapshot_state_bytes,
                snapshot_record_bytes,
                baseline_us,
            )
        } else {
            let (
                proposed_us,
                snapshot_prepare_elapsed_us,
                snapshot_state_bytes,
                snapshot_record_bytes,
            ) = write_proposed(&mut proposed)?;
            let baseline_us = write_baseline(&mut baseline)?;
            (
                proposed_us,
                snapshot_prepare_elapsed_us,
                snapshot_state_bytes,
                snapshot_record_bytes,
                baseline_us,
            )
        };
        proposed_handoff_us.push(proposed_us);
        snapshot_prepare_us.push(snapshot_prepare_elapsed_us);
        baseline_handoff_us.push(baseline_us);
        latest_snapshot_state_json_bytes = snapshot_state_bytes;
        latest_snapshot_record_jsonl_bytes = snapshot_record_bytes;
        latest_snapshot_event_id = snapshot_event_id;
        latest_document_digest = document_digest;
        projection = after_handoff;

        let finished = build_domain_batch(
            &stream_id,
            projection.stream_version,
            1,
            1,
            &mut ids,
            |_| {
                Ok(vec![SessionEvent::ActivationFinished {
                    activation_id: identity.activation_id,
                    outcome: ActivationOutcome::Finished,
                    finished_at_ms: i64::try_from(workload.main_turns + handoff_number)?,
                }])
            },
        )?;
        append_same_domain(&mut proposed, &mut baseline, &finished)?;
        apply_batch(&mut projection, &finished)?;
    }

    let suffix_end = workload
        .main_turns
        .checked_add(workload.suffix_turns)
        .context("suffix boundary overflow")?;
    append_turn_range(
        &mut proposed,
        &mut baseline,
        &mut projection,
        &mut ids,
        &selection,
        next_turn,
        suffix_end,
        workload,
    )?;

    if !projection.state.pending_mailbox.is_empty()
        || projection.state.active_activation.is_some()
        || projection.state.active_model_round.is_some()
    {
        bail!("workload did not finish in an idle compact state");
    }
    let latest_handoff = projection
        .state
        .latest_handoff
        .as_ref()
        .context("workload did not create a handoff")?;
    let expected = Expected {
        token_scenario: token_scenario.clone(),
        handoffs,
        stream_id: stream_id.clone(),
        stream_version: projection.stream_version,
        last_domain_event_id: projection.last_domain_event_id.clone(),
        state: projection.state.clone(),
        latest_handoff_id: latest_handoff.event.event_id.clone(),
        latest_document_digest,
        latest_snapshot_event_id,
    };
    fs::write(
        root.join(EXPECTED_FILE),
        serde_json::to_vec_pretty(&expected)?,
    )?;

    if proposed.domain_jsonl_bytes != baseline.domain_jsonl_bytes {
        bail!("baseline and snapshot domain bytes differ");
    }
    let proposed_stats = proposed.finish()?;
    let baseline_stats = baseline.finish()?;
    let full = recover_full(&baseline_root, &stream_id)?;
    let from_snapshot = recover_from_snapshot(&proposed_root, &stream_id)?;
    validate_recovery(&full.projection, &expected)?;
    validate_recovery(&from_snapshot.projection, &expected)?;
    validate_latest_handoff_lookup(&proposed_root, &expected)?;

    let logical_delta = proposed_stats
        .logical_jsonl_bytes
        .checked_sub(baseline_stats.logical_jsonl_bytes)
        .context("snapshot logical size is below baseline")?;
    let physical_delta = i128::from(proposed_stats.physical_segment_bytes)
        - i128::from(baseline_stats.physical_segment_bytes);
    let result = GenerateResult {
        token_scenario,
        handoffs,
        main_turns: workload.main_turns,
        suffix_turns: workload.suffix_turns,
        handoff_document_bytes: workload.handoff_document_bytes,
        snapshot_state_json_bytes: latest_snapshot_state_json_bytes,
        snapshot_record_jsonl_bytes: latest_snapshot_record_jsonl_bytes,
        snapshot_prepare_latency: latency_summary(snapshot_prepare_us),
        proposed_handoff_latency: latency_summary(proposed_handoff_us),
        baseline_handoff_latency: latency_summary(baseline_handoff_us),
        logical_amplification: proposed_stats.logical_jsonl_bytes as f64
            / baseline_stats.logical_jsonl_bytes as f64,
        physical_amplification: proposed_stats.physical_segment_bytes as f64
            / baseline_stats.physical_segment_bytes as f64,
        logical_bytes_per_handoff: logical_delta as f64 / handoffs as f64,
        physical_bytes_per_handoff: physical_delta as f64 / handoffs as f64,
        proposed: proposed_stats,
        baseline: baseline_stats,
    };
    fs::write(root.join(RESULT_FILE), serde_json::to_vec_pretty(&result)?)?;
    Ok(result)
}

fn append_turn_range(
    proposed: &mut SegmentWriter,
    baseline: &mut SegmentWriter,
    projection: &mut ProjectionAt,
    ids: &mut OrderedIds,
    selection: &SessionSelection,
    first_turn: u64,
    last_turn: u64,
    workload: Workload,
) -> Result<()> {
    if first_turn > last_turn {
        return Ok(());
    }
    let mut next = first_turn;
    while next <= last_turn {
        let end = last_turn.min(next + workload.turns_per_batch - 1);
        let event_count = usize::try_from((end - next + 1) * 9)?;
        let batch = build_domain_batch(
            &projection.stream_id,
            projection.stream_version,
            event_count,
            u32::try_from(event_count)?,
            ids,
            |event_ids| {
                let mut events = Vec::with_capacity(event_count);
                for (turn_index, turn) in (next..=end).enumerate() {
                    let start = turn_index * 9;
                    events.extend(conversation_turn(
                        turn,
                        selection,
                        workload,
                        &event_ids[start..start + 9],
                    )?);
                }
                Ok(events)
            },
        )?;
        append_same_domain(proposed, baseline, &batch)?;
        apply_batch(projection, &batch)?;
        next = end + 1;
    }
    Ok(())
}

struct HandoffIdentity {
    activation_id: String,
    round_id: String,
    request_id: String,
    attempt_id: String,
}

fn handoff_preparation(
    stream_id: &str,
    projection: &ProjectionAt,
    selection: &SessionSelection,
    handoff_number: u64,
    max_output_tokens: u32,
    ids: &mut OrderedIds,
) -> Result<(DomainBatch, ContextHandoffPlan, HandoffIdentity)> {
    let boundary = projection
        .state
        .history_head
        .as_ref()
        .context("handoff preparation has no history head")?;
    let previous_handoff_id = projection
        .state
        .latest_handoff
        .as_ref()
        .map(|handoff| handoff.event.event_id.clone());
    let timestamp = i64::try_from(100_000 + handoff_number)?;
    let mut plan = None;
    let mut identity = None;
    let batch = build_domain_batch(
        stream_id,
        projection.stream_version,
        5,
        5,
        ids,
        |event_ids| {
            let activation_id = event_ids[0].clone();
            let plan_id = event_ids[1].clone();
            let round_id = event_ids[2].clone();
            let request_id = event_ids[3].clone();
            let attempt_id = event_ids[4].clone();
            let built_plan = ContextHandoffPlan {
                plan_id,
                activation_id: activation_id.clone(),
                previous_handoff_id: previous_handoff_id.clone(),
                next_generation: handoff_number + 1,
                covered_through_message_id: boundary.message_id.clone(),
                max_output_tokens,
                selection: selection.clone(),
            };
            plan = Some(built_plan.clone());
            identity = Some(HandoffIdentity {
                activation_id: activation_id.clone(),
                round_id: round_id.clone(),
                request_id: request_id.clone(),
                attempt_id: attempt_id.clone(),
            });
            Ok(vec![
                SessionEvent::ActivationStarted {
                    activation_id: activation_id.clone(),
                    selection: selection.clone(),
                    started_at_ms: timestamp,
                },
                SessionEvent::ContextHandoffPlanned { plan: built_plan },
                SessionEvent::ModelRoundStarted {
                    activation_id: activation_id.clone(),
                    round_id: round_id.clone(),
                    purpose: ModelRequestPurpose::ContextHandoff,
                    mailbox_through_seq: projection.state.consumed_through_mailbox_seq,
                    started_at_ms: timestamp,
                },
                SessionEvent::ModelRequestDeclared {
                    activation_id: activation_id.clone(),
                    round_id: round_id.clone(),
                    request_id: request_id.clone(),
                    request_fingerprint: format!("handoff-request-fingerprint-{handoff_number:04}"),
                    prompt_fingerprint: format!("handoff-prompt-fingerprint-{handoff_number:04}"),
                    tool_schema_fingerprint: "no-tools".to_owned(),
                    maximum_attempts: 1,
                },
                SessionEvent::ModelAttemptStarted {
                    activation_id,
                    round_id,
                    request_id,
                    attempt_id,
                    attempt_number: 1,
                    started_at_ms: timestamp,
                },
            ])
        },
    )?;
    Ok((
        batch,
        plan.context("handoff plan was not built")?,
        identity.context("handoff identity was not built")?,
    ))
}

fn simulated_user_provider_message(turn: u64, workload: Workload) -> ProviderMessage {
    ProviderMessage {
        role: TranscriptRole::User,
        content: deterministic_content(turn.wrapping_mul(2), workload.user_message_bytes).into(),
        is_error: false,
        tool_call_id: None,
        tool_calls: Vec::new(),
        provider_context: None,
    }
}

fn simulated_assistant_provider_message(turn: u64, workload: Workload) -> ProviderMessage {
    ProviderMessage {
        role: TranscriptRole::Assistant,
        content: deterministic_content(
            turn.wrapping_mul(2).wrapping_add(1),
            workload.assistant_message_bytes,
        )
        .into(),
        is_error: false,
        tool_call_id: None,
        tool_calls: Vec::new(),
        provider_context: None,
    }
}

fn simulated_user_message(turn: u64, workload: Workload, message_id: String) -> TranscriptMessage {
    TranscriptMessage {
        message_id,
        role: TranscriptRole::User,
        content: deterministic_content(turn.wrapping_mul(2), workload.user_message_bytes).into(),
        is_error: false,
        tool_call_id: None,
        tool_calls: Vec::new(),
        provider_context: None,
        source_mailbox_seq: Some(turn),
    }
}

fn simulated_assistant_message(
    turn: u64,
    workload: Workload,
    message_id: String,
) -> TranscriptMessage {
    TranscriptMessage {
        message_id,
        role: TranscriptRole::Assistant,
        content: deterministic_content(
            turn.wrapping_mul(2).wrapping_add(1),
            workload.assistant_message_bytes,
        )
        .into(),
        is_error: false,
        tool_call_id: None,
        tool_calls: Vec::new(),
        provider_context: None,
        source_mailbox_seq: None,
    }
}

fn conversation_turn(
    turn: u64,
    selection: &SessionSelection,
    workload: Workload,
    event_ids: &[String],
) -> Result<Vec<SessionEvent>> {
    if event_ids.len() != 9 {
        bail!("conversation turn requires nine Event ULIDs");
    }
    let timestamp = i64::try_from(turn)?;
    let user_message = simulated_user_message(turn, workload, event_ids[0].clone());
    let assistant_message = simulated_assistant_message(turn, workload, event_ids[7].clone());
    let activation_id = event_ids[1].clone();
    let round_id = event_ids[3].clone();
    let request_id = event_ids[4].clone();
    let attempt_id = event_ids[5].clone();
    Ok(vec![
        SessionEvent::MailboxMessageAppended {
            message: MailboxMessage {
                message_id: user_message.message_id,
                mailbox_seq: turn,
                content: user_message.content,
                received_at_ms: timestamp,
            },
        },
        SessionEvent::ActivationStarted {
            activation_id: activation_id.clone(),
            selection: selection.clone(),
            started_at_ms: timestamp,
        },
        SessionEvent::MailboxDrained {
            through_mailbox_seq: turn,
        },
        SessionEvent::ModelRoundStarted {
            activation_id: activation_id.clone(),
            round_id: round_id.clone(),
            purpose: ModelRequestPurpose::Conversation,
            mailbox_through_seq: turn,
            started_at_ms: timestamp,
        },
        SessionEvent::ModelRequestDeclared {
            activation_id: activation_id.clone(),
            round_id: round_id.clone(),
            request_id: request_id.clone(),
            request_fingerprint: format!("request-fingerprint-{turn:05}"),
            prompt_fingerprint: format!("prompt-fingerprint-{turn:05}"),
            tool_schema_fingerprint: "no-tools".to_owned(),
            maximum_attempts: 1,
        },
        SessionEvent::ModelAttemptStarted {
            activation_id: activation_id.clone(),
            round_id: round_id.clone(),
            request_id: request_id.clone(),
            attempt_id: attempt_id.clone(),
            attempt_number: 1,
            started_at_ms: timestamp,
        },
        SessionEvent::ModelRequestCompleted {
            activation_id: activation_id.clone(),
            round_id,
            request_id,
            attempt_id,
            usage: None,
            provider_input: None,
        },
        SessionEvent::MessageAppended {
            message: assistant_message,
            wake_wait: false,
        },
        SessionEvent::ActivationFinished {
            activation_id,
            outcome: ActivationOutcome::Finished,
            finished_at_ms: timestamp,
        },
    ])
}

fn build_domain_batch(
    stream_id: &str,
    current_version: u64,
    event_count: usize,
    persisted_batch_size: u32,
    ids: &mut OrderedIds,
    build: impl FnOnce(&[String]) -> Result<Vec<SessionEvent>>,
) -> Result<DomainBatch> {
    if event_count == 0 {
        bail!("domain batch is empty");
    }
    let event_ids = (0..event_count).map(|_| ids.next()).collect::<Vec<_>>();
    let events = build(&event_ids)?;
    if events.len() != event_count {
        bail!("domain batch builder returned the wrong event count");
    }
    if persisted_batch_size < u32::try_from(event_count)? {
        bail!("persisted batch is smaller than its domain events");
    }
    let mut records = Vec::with_capacity(event_count);
    for (index, (event_id, event)) in event_ids.into_iter().zip(events).enumerate() {
        event.validate()?;
        if created_object_id(&event).is_some_and(|object_id| object_id != event_id) {
            bail!("event-created object does not use its Event ULID");
        }
        records.push(DomainRecord {
            event_id,
            stream_id: stream_id.to_owned(),
            stream_version: current_version
                .checked_add(u64::try_from(index + 1)?)
                .context("stream version overflow")?,
            event_schema_version: EVENT_SCHEMA_VERSION,
            batch_index: u32::try_from(index)?,
            batch_size: persisted_batch_size,
            event,
        });
    }
    let stored = records
        .iter()
        .map(|record| StoredRecord::Domain {
            event_id: record.event_id.clone(),
            stream_id: record.stream_id.clone(),
            stream_version: record.stream_version,
            event_schema_version: record.event_schema_version,
            batch_index: record.batch_index,
            batch_size: record.batch_size,
            event: Box::new(record.event.clone()),
        })
        .collect::<Vec<_>>();
    Ok(DomainBatch {
        records,
        bytes: serialize_records(&stored)?,
    })
}

fn created_object_id(event: &SessionEvent) -> Option<&str> {
    match event {
        SessionEvent::SessionCreated { session_id, .. } => Some(session_id),
        SessionEvent::MailboxMessageAppended { message } => Some(&message.message_id),
        SessionEvent::MessageAppended { message, .. } => Some(&message.message_id),
        SessionEvent::ActivationStarted { activation_id, .. } => Some(activation_id),
        SessionEvent::ModelRoundStarted { round_id, .. } => Some(round_id),
        SessionEvent::ContextHandoffPlanned { plan } => Some(&plan.plan_id),
        SessionEvent::ContextHandoffCreated { handoff } => Some(&handoff.handoff_id),
        SessionEvent::ModelRequestDeclared { request_id, .. } => Some(request_id),
        SessionEvent::ModelAttemptStarted { attempt_id, .. } => Some(attempt_id),
        SessionEvent::WaitSet { wait } => Some(&wait.wait_id),
        _ => None,
    }
}

fn append_same_domain(
    proposed: &mut SegmentWriter,
    baseline: &mut SegmentWriter,
    batch: &DomainBatch,
) -> Result<()> {
    proposed.append_domain(batch)?;
    baseline.append_domain(batch)?;
    Ok(())
}

fn apply_batch(projection: &mut ProjectionAt, batch: &DomainBatch) -> Result<()> {
    for record in &batch.records {
        projection.apply(record)?;
    }
    Ok(())
}

fn serialize_records(records: &[StoredRecord]) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    for record in records {
        serde_json::to_writer(&mut bytes, record)?;
        bytes.push(b'\n');
    }
    Ok(bytes)
}

fn validate_compact_snapshot_json(bytes: &[u8]) -> Result<()> {
    let value: Value = serde_json::from_slice(
        bytes
            .strip_suffix(b"\n")
            .context("snapshot JSONL has no trailing newline")?,
    )?;
    for forbidden in ["transcript", "document", "content"] {
        if contains_object_key(&value, forbidden) {
            bail!("compact snapshot contains forbidden key {forbidden}");
        }
    }
    Ok(())
}

fn contains_object_key(value: &Value, target: &str) -> bool {
    match value {
        Value::Object(object) => {
            object.contains_key(target)
                || object
                    .values()
                    .any(|value| contains_object_key(value, target))
        }
        Value::Array(values) => values
            .iter()
            .any(|value| contains_object_key(value, target)),
        _ => false,
    }
}

fn rehydrate_command(root: &Path, mode: &str) -> Result<()> {
    let expected = read_expected(root)?;
    let started = Instant::now();
    let recovery = match mode {
        "snapshot" => recover_from_snapshot(&root.join("snapshot"), &expected.stream_id)?,
        "baseline" => recover_full(&root.join("baseline"), &expected.stream_id)?,
        _ => bail!("recovery mode must be snapshot or baseline"),
    };
    let elapsed_us = duration_us(started.elapsed())?;
    validate_recovery(&recovery.projection, &expected)?;
    let result = RecoveryResult {
        context_window_tokens: expected
            .token_scenario
            .as_ref()
            .map(|scenario| scenario.context_window_tokens),
        handoffs: expected.handoffs,
        mode: mode.to_owned(),
        elapsed_us,
        peak_rss_bytes: peak_rss_bytes()?,
        physical_bytes_read: recovery.physical_bytes_read,
        logical_bytes_read: recovery.logical_bytes_read,
        domain_events_applied: recovery.domain_events_applied,
        stream_version: recovery.projection.stream_version,
    };
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}

fn lookup_command(root: &Path) -> Result<()> {
    let expected = read_expected(root)?;
    let started = Instant::now();
    let (segment_id, read, document) = lookup_handoff(&root.join("snapshot"), &expected)?;
    let elapsed_us = duration_us(started.elapsed())?;
    let result = LookupResult {
        context_window_tokens: expected
            .token_scenario
            .as_ref()
            .map(|scenario| scenario.context_window_tokens),
        handoffs: expected.handoffs,
        elapsed_us,
        peak_rss_bytes: peak_rss_bytes()?,
        physical_bytes_read: read.physical_bytes,
        logical_bytes_read: read.logical_bytes,
        segment_id,
        handoff_id: expected.latest_handoff_id,
        document_bytes: document.len(),
    };
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}

fn read_expected(root: &Path) -> Result<Expected> {
    Ok(serde_json::from_slice(&fs::read(
        root.join(EXPECTED_FILE),
    )?)?)
}

fn recover_full(root: &Path, stream_id: &str) -> Result<Recovery> {
    let paths = segment_paths(root)?;
    let mut projection = ProjectionAt::empty(stream_id);
    let mut physical_bytes_read = 0_u64;
    let mut logical_bytes_read = 0_u64;
    let mut domain_events_applied = 0_u64;
    for path in paths {
        let read = read_segment(&path)?;
        physical_bytes_read = physical_bytes_read
            .checked_add(read.physical_bytes)
            .context("physical read overflow")?;
        logical_bytes_read = logical_bytes_read
            .checked_add(read.logical_bytes)
            .context("logical read overflow")?;
        for record in read.records {
            if let Some(record) = record.domain_record() {
                projection.apply(&record)?;
                domain_events_applied += 1;
            }
        }
    }
    Ok(Recovery {
        projection,
        physical_bytes_read,
        logical_bytes_read,
        domain_events_applied,
    })
}

fn recover_from_snapshot(root: &Path, stream_id: &str) -> Result<Recovery> {
    let current = segment_paths(root)?
        .into_iter()
        .next_back()
        .context("snapshot stream has no segment")?;
    if is_compressed(&current) {
        bail!("current segment is compressed");
    }
    let read = read_snapshot_tail(&current)?;
    let snapshot_index = read
        .records
        .iter()
        .rposition(|record| matches!(record, StoredRecord::Snapshot { .. }))
        .context("current segment has no snapshot")?;
    let StoredRecord::Snapshot {
        event_id: snapshot_event_id,
        stream_id: snapshot_stream_id,
        batch_index: snapshot_batch_index,
        batch_size: snapshot_batch_size,
        through_event_id,
        through_stream_version,
        state_schema_version,
        reducer_schema_version,
        state,
    } = &read.records[snapshot_index]
    else {
        unreachable!();
    };
    if snapshot_stream_id != stream_id
        || *state_schema_version != SNAPSHOT_STATE_SCHEMA_VERSION
        || *reducer_schema_version != SNAPSHOT_REDUCER_SCHEMA_VERSION
    {
        bail!("snapshot envelope is invalid");
    }
    let preceding = read.records[..snapshot_index]
        .iter()
        .rev()
        .filter_map(StoredRecord::domain_record)
        .take(2)
        .collect::<Vec<_>>();
    let completed = preceding
        .first()
        .context("snapshot has no preceding completion event")?;
    let handoff = preceding
        .get(1)
        .context("snapshot has no preceding handoff event")?;
    if completed.event_id != *through_event_id
        || completed.stream_version != *through_stream_version
        || completed.batch_index != 1
        || completed.batch_size != 3
        || !matches!(&completed.event, SessionEvent::ModelRequestCompleted { .. })
        || handoff.batch_index != 0
        || handoff.batch_size != 3
        || handoff.stream_version + 1 != completed.stream_version
        || !matches!(&handoff.event, SessionEvent::ContextHandoffCreated { .. })
        || *snapshot_batch_index != 2
        || *snapshot_batch_size != 3
    {
        bail!("snapshot does not complete one handoff append batch");
    }
    if snapshot_event_id <= through_event_id {
        bail!("snapshot event ID is not after the handoff");
    }
    let mut projection = ProjectionAt {
        stream_id: stream_id.to_owned(),
        stream_version: *through_stream_version,
        last_domain_event_id: through_event_id.clone(),
        state: state.as_ref().clone(),
    };
    let mut domain_events_applied = 0_u64;
    for record in &read.records[snapshot_index + 1..] {
        let Some(record) = record.domain_record() else {
            bail!("suffix contains another snapshot");
        };
        projection.apply(&record)?;
        domain_events_applied += 1;
    }
    Ok(Recovery {
        projection,
        physical_bytes_read: read.physical_bytes,
        logical_bytes_read: read.logical_bytes,
        domain_events_applied,
    })
}

fn validate_recovery(projection: &ProjectionAt, expected: &Expected) -> Result<()> {
    if projection.stream_id != expected.stream_id
        || projection.stream_version != expected.stream_version
        || projection.last_domain_event_id != expected.last_domain_event_id
        || projection.state != expected.state
    {
        bail!("recovered projection does not match expected state");
    }
    Ok(())
}

fn validate_latest_handoff_lookup(root: &Path, expected: &Expected) -> Result<()> {
    let (_, _, document) = lookup_handoff(root, expected)?;
    if sha256_hex(document.as_bytes()) != expected.latest_document_digest {
        bail!("looked-up handoff document digest does not match");
    }
    Ok(())
}

fn lookup_handoff(root: &Path, expected: &Expected) -> Result<(String, ReadResult, String)> {
    let paths = segment_paths(root)?;
    let insertion = paths.partition_point(|path| {
        segment_id(path)
            .is_ok_and(|segment_id| segment_id.as_str() <= expected.latest_handoff_id.as_str())
    });
    let index = insertion
        .checked_sub(1)
        .context("no segment starts before the handoff event")?;
    let segment_id = segment_id(&paths[index])?;
    let (read, document) = read_handoff_from_segment(&paths[index], &expected.latest_handoff_id)?;
    if sha256_hex(document.as_bytes()) != expected.latest_document_digest {
        bail!("handoff lookup returned the wrong document");
    }
    Ok((segment_id, read, document))
}

fn read_handoff_from_segment(path: &Path, target_event_id: &str) -> Result<(ReadResult, String)> {
    if is_compressed(path) {
        let read = read_segment(path)?;
        let document = read
            .records
            .iter()
            .find_map(|record| {
                let domain = record.domain_record()?;
                if domain.event_id != target_event_id {
                    return None;
                }
                let SessionEvent::ContextHandoffCreated { handoff } = domain.event else {
                    return None;
                };
                Some(handoff.document)
            })
            .context("handoff document event is absent from the selected segment")?;
        return Ok((read, document));
    }

    let mut reader = BufReader::with_capacity(IO_BLOCK_BYTES, File::open(path)?);
    let mut line = Vec::new();
    let mut logical_bytes = 0_u64;
    loop {
        line.clear();
        let line_bytes = reader.read_until(b'\n', &mut line)?;
        if line_bytes == 0 {
            bail!("handoff document event is absent from the selected segment");
        }
        logical_bytes = logical_bytes
            .checked_add(u64::try_from(line_bytes)?)
            .context("handoff lookup byte count overflow")?;
        let json = line
            .strip_suffix(b"\n")
            .context("segment record has no trailing newline")?;
        let record: StoredRecord = serde_json::from_slice(json)?;
        match record.event_id().cmp(target_event_id) {
            Ordering::Less => continue,
            Ordering::Greater => {
                bail!("selected segment passed the handoff event ID without finding it")
            }
            Ordering::Equal => {
                let StoredRecord::Domain { event, .. } = record else {
                    bail!("handoff event ID resolves to a snapshot");
                };
                let SessionEvent::ContextHandoffCreated { handoff } = *event else {
                    bail!("handoff event ID resolves to another domain event");
                };
                let physical_bytes = reader.get_mut().stream_position()?;
                return Ok((
                    ReadResult {
                        records: Vec::new(),
                        physical_bytes,
                        logical_bytes,
                    },
                    handoff.document,
                ));
            }
        }
    }
}

fn segment_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let directory = root.join("segments");
    let mut paths = fs::read_dir(&directory)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    paths.retain(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".jsonl") || name.ends_with(".jsonl.zst"))
    });
    paths.sort_by_key(|path| segment_id(path).unwrap_or_default());
    Ok(paths)
}

fn segment_id(path: &Path) -> Result<String> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("segment name is not UTF-8")?;
    name.strip_suffix(".jsonl.zst")
        .or_else(|| name.strip_suffix(".jsonl"))
        .map(str::to_owned)
        .context("invalid segment suffix")
}

fn read_snapshot_tail(path: &Path) -> Result<ReadResult> {
    if is_compressed(path) {
        bail!("cannot seek backwards through a compressed current segment");
    }
    let mut file = File::open(path)?;
    let mut cursor = file.metadata()?.len();
    if cursor == 0 {
        bail!("current segment is empty");
    }

    let mut carry = Vec::new();
    let mut reverse_records = Vec::new();
    let mut found_snapshot = false;
    let mut predecessors_remaining = 0_usize;
    let mut bytes_read = 0_u64;

    while cursor > 0 && (!found_snapshot || predecessors_remaining > 0) {
        let block_len = usize::try_from(cursor.min(IO_BLOCK_BYTES as u64))?;
        cursor -= u64::try_from(block_len)?;
        file.seek(SeekFrom::Start(cursor))?;
        let mut block = vec![0; block_len];
        file.read_exact(&mut block)?;
        bytes_read = bytes_read
            .checked_add(u64::try_from(block_len)?)
            .context("snapshot tail read byte count overflow")?;
        block.extend_from_slice(&carry);

        let complete_start = if cursor == 0 {
            0
        } else if let Some(newline) = block.iter().position(|byte| *byte == b'\n') {
            newline + 1
        } else {
            carry = block;
            continue;
        };
        let next_carry = block[..complete_start].to_vec();

        for line in block[complete_start..].split(|byte| *byte == b'\n').rev() {
            if line.is_empty() {
                continue;
            }
            let record: StoredRecord = serde_json::from_slice(line)?;
            if found_snapshot {
                reverse_records.push(record);
                predecessors_remaining = predecessors_remaining.saturating_sub(1);
                if predecessors_remaining == 0 {
                    break;
                }
                continue;
            }
            if let StoredRecord::Snapshot { batch_index, .. } = &record {
                found_snapshot = true;
                predecessors_remaining = usize::try_from(*batch_index)?;
            }
            reverse_records.push(record);
        }
        carry = next_carry;
    }

    if !found_snapshot {
        bail!("current segment has no snapshot");
    }
    if predecessors_remaining > 0 {
        bail!("snapshot has an incomplete append batch");
    }
    reverse_records.reverse();
    if reverse_records
        .windows(2)
        .any(|pair| pair[0].event_id() >= pair[1].event_id())
    {
        bail!("snapshot tail records are not ordered by event ID");
    }

    Ok(ReadResult {
        records: reverse_records,
        physical_bytes: bytes_read,
        logical_bytes: bytes_read,
    })
}

fn read_segment(path: &Path) -> Result<ReadResult> {
    let physical_bytes = fs::metadata(path)?.len();
    let bytes = if is_compressed(path) {
        let mut decoder = zstd::stream::read::Decoder::new(File::open(path)?)?;
        let mut bytes = Vec::new();
        decoder.read_to_end(&mut bytes)?;
        bytes
    } else {
        fs::read(path)?
    };
    let logical_bytes = u64::try_from(bytes.len())?;
    let mut records = Vec::new();
    for line in bytes.split(|byte| *byte == b'\n') {
        if line.is_empty() {
            continue;
        }
        let record: StoredRecord = serde_json::from_slice(line)?;
        if records
            .last()
            .is_some_and(|previous: &StoredRecord| previous.event_id() >= record.event_id())
        {
            bail!("segment records are not ordered by event ID");
        }
        records.push(record);
    }
    Ok(ReadResult {
        records,
        physical_bytes,
        logical_bytes,
    })
}

fn is_compressed(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".jsonl.zst"))
}

fn compress_segment(path: &Path) -> Result<()> {
    let compressed = compressed_path(path);
    let temporary = compressed.with_extension("zst.tmp");
    let mut source = File::open(path)?;
    let destination = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)?;
    let mut encoder = zstd::stream::write::Encoder::new(destination, 12)?;
    std::io::copy(&mut source, &mut encoder)?;
    let destination = encoder.finish()?;
    destination.sync_all()?;
    drop(destination);
    fs::rename(&temporary, &compressed)?;
    sync_directory(path.parent().context("segment has no parent")?)?;
    fs::remove_file(path)?;
    sync_directory(path.parent().context("segment has no parent")?)?;
    Ok(())
}

fn compressed_path(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.zst", path.display()))
}

fn sync_directory(path: &Path) -> Result<()> {
    File::open(path)?.sync_all()?;
    Ok(())
}

fn latency_summary(mut values: Vec<u64>) -> LatencySummary {
    if values.is_empty() {
        return LatencySummary {
            samples: 0,
            total_us: 0,
            min_us: 0,
            median_us: 0,
            p95_us: 0,
            max_us: 0,
        };
    }
    values.sort_unstable();
    let samples = values.len();
    let percentile_index = |numerator: usize, denominator: usize| {
        (samples * numerator)
            .div_ceil(denominator)
            .saturating_sub(1)
    };
    LatencySummary {
        samples,
        total_us: values.iter().sum(),
        min_us: values[0],
        median_us: values[percentile_index(50, 100)],
        p95_us: values[percentile_index(95, 100)],
        max_us: values[samples - 1],
    }
}

fn deterministic_content(seed: u64, bytes: usize) -> String {
    let mut state = seed ^ 0x9E37_79B9_7F4A_7C15;
    let mut output = String::with_capacity(bytes);
    for _ in 0..bytes {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let value = 32 + u8::try_from(state % 95).unwrap_or_default();
        output.push(char::from(value));
    }
    output
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn duration_us(duration: Duration) -> Result<u64> {
    u64::try_from(duration.as_micros()).context("duration exceeds u64 microseconds")
}

fn peak_rss_bytes() -> Result<u64> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
    let result = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
    if result != 0 {
        bail!("getrusage failed");
    }
    let usage = unsafe { usage.assume_init() };
    #[cfg(target_os = "macos")]
    {
        u64::try_from(usage.ru_maxrss).context("peak RSS is negative")
    }
    #[cfg(not(target_os = "macos"))]
    {
        u64::try_from(usage.ru_maxrss)
            .context("peak RSS is negative")?
            .checked_mul(1024)
            .context("peak RSS overflow")
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    const TEST_WORKLOAD: Workload = Workload {
        main_turns: 14,
        suffix_turns: 7,
        turns_per_batch: 7,
        user_message_bytes: 64,
        assistant_message_bytes: 128,
        handoff_document_bytes: 256,
        segment_target_bytes: 1_024,
    };

    const TAIL_READ_WORKLOAD: Workload = Workload {
        main_turns: 128,
        suffix_turns: 2,
        turns_per_batch: 7,
        user_message_bytes: 1_024,
        assistant_message_bytes: 2_048,
        handoff_document_bytes: 256,
        segment_target_bytes: 64 * MIB,
    };

    #[test]
    fn handoff_and_compact_snapshot_share_the_new_segment() {
        let root = tempdir().unwrap();
        let result = generate(root.path(), 1, TEST_WORKLOAD, None).unwrap();
        assert_eq!(result.proposed.snapshot_count, 1);
        assert_eq!(
            result.proposed.durable_append_count,
            result.baseline.durable_append_count
        );
        assert!(result.proposed.segment_count >= 2);
        let expected = read_expected(root.path()).unwrap();
        let paths = segment_paths(&root.path().join("snapshot")).unwrap();
        let current = paths.last().unwrap();
        let read = read_segment(current).unwrap();
        let handoff_index = read
            .records
            .iter()
            .position(|record| record.event_id() == expected.latest_handoff_id)
            .unwrap();
        let snapshot_index = read
            .records
            .iter()
            .position(|record| record.event_id() == expected.latest_snapshot_event_id)
            .unwrap();
        assert_eq!(snapshot_index, handoff_index + 2);
        assert!(matches!(
            &read.records[handoff_index + 1],
            StoredRecord::Domain {
                batch_index: 1,
                batch_size: 3,
                event,
                ..
            } if matches!(event.as_ref(), SessionEvent::ModelRequestCompleted { .. })
        ));
        assert!(matches!(
            &read.records[snapshot_index],
            StoredRecord::Snapshot {
                batch_index: 2,
                batch_size: 3,
                ..
            }
        ));
        let snapshot_bytes = serialize_records(&[read.records[snapshot_index].clone()]).unwrap();
        validate_compact_snapshot_json(&snapshot_bytes).unwrap();
    }

    #[test]
    fn snapshot_suffix_recovery_matches_full_replay() {
        let root = tempdir().unwrap();
        generate(root.path(), 1, TEST_WORKLOAD, None).unwrap();
        let expected = read_expected(root.path()).unwrap();
        let full = recover_full(&root.path().join("baseline"), &expected.stream_id).unwrap();
        let snapshot =
            recover_from_snapshot(&root.path().join("snapshot"), &expected.stream_id).unwrap();
        assert_eq!(full.projection, snapshot.projection);
        validate_recovery(&snapshot.projection, &expected).unwrap();
    }

    #[test]
    fn recovery_skips_pre_snapshot_history_and_lookup_stops_at_target() {
        let root = tempdir().unwrap();
        let result = generate(root.path(), 1, TAIL_READ_WORKLOAD, None).unwrap();
        let expected = read_expected(root.path()).unwrap();
        let snapshot =
            recover_from_snapshot(&root.path().join("snapshot"), &expected.stream_id).unwrap();
        assert!(snapshot.physical_bytes_read * 2 < result.proposed.current_segment_bytes);

        let (_, lookup, _) = lookup_handoff(&root.path().join("snapshot"), &expected).unwrap();
        assert!(lookup.logical_bytes < result.proposed.current_segment_bytes);
    }

    #[test]
    fn context_workload_is_calibrated_in_tokens_at_the_runtime_budget() {
        let mut previous_turns = 0;
        for context_window_tokens in TOKEN_SCENARIO_WINDOWS {
            let calibrated = calibrate_context_workload(context_window_tokens).unwrap();
            if context_window_tokens == 100_000 {
                assert_eq!(calibrated.scenario.normal_input_budget_tokens, 59_808);
            }
            assert!(
                calibrated.scenario.previous_input_tokens
                    <= calibrated.scenario.normal_input_budget_tokens
            );
            assert!(
                calibrated.scenario.estimated_input_tokens
                    > calibrated.scenario.normal_input_budget_tokens
            );
            assert!(calibrated.scenario.overshoot_tokens < calibrated.scenario.last_turn_tokens);
            assert!(calibrated.scenario.provider_message_count <= 8_192);
            assert_eq!(
                calibrated.scenario.handoff_input_budget_tokens,
                context_window_tokens - u64::from(TOKEN_SCENARIO_MAX_OUTPUT_TOKENS)
            );
            assert!(
                calibrated.scenario.selected_boundary_handoff_source_tokens
                    <= calibrated.scenario.handoff_input_budget_tokens
            );
            assert_eq!(
                calibrated.scenario.selected_boundary_handoff_source_tokens,
                calibrated.scenario.latest_boundary_handoff_source_tokens
            );
            assert_eq!(calibrated.scenario.tail_message_count, 0);
            assert_eq!(
                calibrated.scenario.selected_boundary_message_index + 2,
                calibrated.scenario.provider_message_count
            );
            assert!(
                calibrated
                    .scenario
                    .next_generation_after_suffix_input_tokens
                    <= calibrated.scenario.normal_input_budget_tokens
            );
            assert!(calibrated.workload.main_turns > previous_turns);
            assert_eq!(calibrated.workload.handoff_document_bytes, 16_128);
            previous_turns = calibrated.workload.main_turns;
        }
    }
}
