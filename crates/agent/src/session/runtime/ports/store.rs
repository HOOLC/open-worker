use std::{ops::Deref, sync::Arc};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::session::state::{
    DomainError, EventDraft, EventRecord, SessionSelection, SessionState, StreamVersion,
};

pub const MAX_SESSION_LIST_LIMIT: usize = 200;

const SESSION_LIST_CURSOR_SCHEMA: &str = "zork.session-list-cursor.v1";
const SESSION_LIST_CURSOR_ROUTE: &str = "/v1/sessions";
const SESSION_LIST_CURSOR_SORT_VERSION: u32 = 1;
const MAX_SESSION_LIST_CURSOR_BYTES: usize = 4 * 1024;
pub(crate) const INTEGRITY_DIGEST_BYTES: usize = 32;
pub(crate) type IntegrityDigest = [u8; INTEGRITY_DIGEST_BYTES];

#[derive(Clone, Debug, PartialEq)]
pub struct AppendResult {
    pub stream_id: String,
    pub events: Vec<EventRecord>,
    pub stream_version: StreamVersion,
    pub replayed: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SessionAppendResult {
    pub append: AppendResult,
    pub state: VerifiedSessionState,
}

/// One immutable session projection bound to the integrity anchor that proved
/// it.  The fields are private so callers cannot pair a valid proof with a
/// modified projection; a new value can only come from a successful storage
/// read or append.
#[derive(Clone, Debug, PartialEq)]
pub struct VerifiedSessionState {
    pub(crate) state: Arc<SessionState>,
    pub(crate) prefix_digest: Vec<u8>,
    pub(crate) state_digest_version: i64,
    pub(crate) state_digest: Vec<u8>,
    pub(crate) digest_components: StateDigestComponents,
}

impl VerifiedSessionState {
    pub fn into_state(self) -> SessionState {
        Arc::try_unwrap(self.state).unwrap_or_else(|state| (*state).clone())
    }
}

impl Deref for VerifiedSessionState {
    type Target = SessionState;

    fn deref(&self) -> &Self::Target {
        self.state.as_ref()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SessionCreate {
    pub created_at_ms: i64,
    pub selection: SessionSelection,
    pub system_prompt: Option<String>,
    pub workspace: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SessionCreateResult {
    pub append: AppendResult,
    pub state: SessionState,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SessionListItem {
    pub session_id: String,
    pub version: StreamVersion,
    pub status: String,
    pub created_at_ms: i64,
    pub selection: SessionSelection,
    pub workspace: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionListCursor {
    created_at_ms: i64,
    session_id: String,
}

impl SessionListCursor {
    pub fn new(created_at_ms: i64, session_id: impl Into<String>) -> Result<Self, StoreError> {
        if created_at_ms < 0 {
            return Err(StoreError::InvalidSessionListCursor);
        }
        let session_id = session_id.into();
        require_text("session_id", &session_id)?;
        Ok(Self {
            created_at_ms,
            session_id,
        })
    }

    pub fn created_at_ms(&self) -> i64 {
        self.created_at_ms
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn encode(&self) -> Result<String, StoreError> {
        let wire = SessionListCursorWire {
            schema: SESSION_LIST_CURSOR_SCHEMA.to_owned(),
            route: SESSION_LIST_CURSOR_ROUTE.to_owned(),
            sort_version: SESSION_LIST_CURSOR_SORT_VERSION,
            created_at_ms: self.created_at_ms,
            session_id: self.session_id.clone(),
        };
        let bytes = canonical_json_bytes(&wire)?;
        if bytes.len() > MAX_SESSION_LIST_CURSOR_BYTES {
            return Err(StoreError::InvalidSessionListCursor);
        }
        Ok(format!("zsc1.{}", hex_encode(&bytes)))
    }

    pub fn decode(encoded: &str) -> Result<Self, StoreError> {
        let encoded = encoded
            .strip_prefix("zsc1.")
            .ok_or(StoreError::InvalidSessionListCursor)?;
        let bytes = hex_decode(encoded).ok_or(StoreError::InvalidSessionListCursor)?;
        if bytes.is_empty() || bytes.len() > MAX_SESSION_LIST_CURSOR_BYTES {
            return Err(StoreError::InvalidSessionListCursor);
        }
        let wire: SessionListCursorWire =
            serde_json::from_slice(&bytes).map_err(|_| StoreError::InvalidSessionListCursor)?;
        if wire.schema != SESSION_LIST_CURSOR_SCHEMA
            || wire.route != SESSION_LIST_CURSOR_ROUTE
            || wire.sort_version != SESSION_LIST_CURSOR_SORT_VERSION
        {
            return Err(StoreError::InvalidSessionListCursor);
        }
        Self::new(wire.created_at_ms, wire.session_id)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SessionListCursorWire {
    schema: String,
    route: String,
    sort_version: u32,
    created_at_ms: i64,
    session_id: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SessionListPage {
    pub items: Vec<SessionListItem>,
    pub next_cursor: Option<SessionListCursor>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionRef {
    pub session_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutstandingWaitTimer {
    pub session_id: String,
    pub wait_id: String,
    pub deadline_ms: i64,
}

pub trait StorePort: Send + Sync {
    fn create_session(&self, create: &SessionCreate) -> Result<SessionCreateResult, StoreError>;

    fn append(
        &self,
        stream_id: &str,
        current: &SessionState,
        events: &[EventDraft],
    ) -> Result<SessionAppendResult, StoreError>;

    fn append_verified(
        &self,
        stream_id: &str,
        current: VerifiedSessionState,
        events: &[EventDraft],
    ) -> Result<SessionAppendResult, StoreError>;

    fn rehydrate(&self, stream_id: &str) -> Result<SessionState, RehydrateError>;

    fn rehydrate_verified(&self, stream_id: &str) -> Result<VerifiedSessionState, RehydrateError>;

    fn read_stream(
        &self,
        stream_id: &str,
        after_version: StreamVersion,
        limit: usize,
    ) -> Result<Vec<EventRecord>, StoreError>;

    /// Read at most `limit` domain events strictly before an event ULID.
    /// Results are chronological even though storage is traversed backwards.
    fn read_stream_before(
        &self,
        stream_id: &str,
        before_event_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<EventRecord>, StoreError>;

    /// Locate one domain event by its immutable event ULID.
    fn read_event(
        &self,
        stream_id: &str,
        event_id: &str,
    ) -> Result<Option<EventRecord>, StoreError>;

    /// Every outstanding wait timer, including future deadlines.
    ///
    /// The adapter must not filter by the current clock. Startup arms the
    /// complete set; a due fire is decided later by the timer adapter.
    fn list_outstanding_wait_timers(&self) -> Result<Vec<OutstandingWaitTimer>, StoreError>;

    /// Sessions matching `SessionState::is_startup_runnable`.
    fn list_runnable_sessions(&self) -> Result<Vec<SessionRef>, StoreError>;

    /// Sessions with a durable active activation.
    fn list_active_activations(&self) -> Result<Vec<SessionRef>, StoreError>;

    fn list_sessions(&self, limit: usize) -> Result<Vec<SessionListItem>, StoreError>;

    fn list_sessions_page(
        &self,
        cursor: Option<&SessionListCursor>,
        limit: usize,
    ) -> Result<SessionListPage, StoreError>;

    /// Atomically append a successful context-handoff domain batch and the
    /// compact after-state snapshot that terminates that same physical batch.
    fn append_handoff_verified(
        &self,
        stream_id: &str,
        current: VerifiedSessionState,
        events: &[EventDraft],
    ) -> Result<SessionAppendResult, StoreError>;
}

pub use StorePort as EventStore;

#[derive(Debug, Error)]
pub enum StorePortError {
    #[error("storage backend error")]
    Backend,
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("invalid {field}: must not be empty")]
    EmptyField { field: &'static str },
    #[error("event append must contain at least one event")]
    EmptyEventBatch,
    #[error("value for {field} exceeds the supported range")]
    IntegerRange { field: &'static str },
    #[error(
        "optimistic concurrency conflict on stream {stream_id}: expected version {expected}, actual {actual}"
    )]
    OptimisticConcurrency {
        stream_id: String,
        expected: StreamVersion,
        actual: StreamVersion,
    },
    #[error("event {event_id} was already stored with a different batch")]
    EventBatchConflict { event_id: String },
    #[error("event stream projection error: {0}")]
    Domain(#[from] DomainError),
    #[error("stream integrity metadata is invalid for {stream_id} at version {version}")]
    InvalidIntegrityAnchor {
        stream_id: String,
        version: StreamVersion,
    },
    #[error("snapshot state does not match the append-time integrity anchor")]
    SnapshotStateMismatch,
    #[error("rehydration integrity check failed for {stream_id} at version {version}")]
    RehydrationIntegrity {
        stream_id: String,
        version: StreamVersion,
    },
    #[error("event store mutex was poisoned")]
    Poisoned,
    #[error("session was not found")]
    SessionNotFound,
    #[error("session list limit must be between 1 and {MAX_SESSION_LIST_LIMIT}")]
    InvalidSessionListLimit,
    #[error("session list cursor is malformed")]
    InvalidSessionListCursor,
    #[error("event id or event cursor is malformed")]
    InvalidEventId,
    #[error("session stream is inconsistent with its creation event")]
    InvalidSessionStream,
}

pub use StorePortError as StoreError;

#[derive(Debug, Error)]
pub enum RehydrateError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Domain(#[from] DomainError),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StateDigestComponents {
    pub(crate) transcript: IntegrityDigest,
}

pub(crate) fn require_text(field: &'static str, value: &str) -> Result<(), StoreError> {
    if value.is_empty() {
        Err(StoreError::EmptyField { field })
    } else {
        Ok(())
    }
}

pub(crate) fn hash_field(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

pub(crate) fn canonical_json_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, StoreError> {
    let value = serde_json::to_value(value)?;
    let mut bytes = Vec::new();
    write_canonical_json(&value, &mut bytes)?;
    Ok(bytes)
}

fn write_canonical_json(value: &Value, output: &mut Vec<u8>) -> Result<(), StoreError> {
    match value {
        Value::Null => output.extend_from_slice(b"null"),
        Value::Bool(value) => output.extend_from_slice(if *value { b"true" } else { b"false" }),
        Value::Number(value) => output.extend_from_slice(value.to_string().as_bytes()),
        Value::String(value) => output.extend_from_slice(&serde_json::to_vec(value)?),
        Value::Array(values) => {
            output.push(b'[');
            for (index, value) in values.iter().enumerate() {
                if index != 0 {
                    output.push(b',');
                }
                write_canonical_json(value, output)?;
            }
            output.push(b']');
        }
        Value::Object(values) => {
            let mut entries = values.iter().collect::<Vec<_>>();
            entries.sort_unstable_by(|left, right| left.0.cmp(right.0));
            output.push(b'{');
            for (index, (key, value)) in entries.into_iter().enumerate() {
                if index != 0 {
                    output.push(b',');
                }
                output.extend_from_slice(&serde_json::to_vec(key)?);
                output.push(b':');
                write_canonical_json(value, output)?;
            }
            output.push(b'}');
        }
    }
    Ok(())
}

pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn hex_decode(encoded: &str) -> Option<Vec<u8>> {
    if encoded.is_empty()
        || !encoded.len().is_multiple_of(2)
        || encoded.len() / 2 > MAX_SESSION_LIST_CURSOR_BYTES
    {
        return None;
    }
    let mut decoded = Vec::with_capacity(encoded.len() / 2);
    let bytes = encoded.as_bytes();
    for pair in bytes.chunks_exact(2) {
        let high = hex_digit(pair[0])?;
        let low = hex_digit(pair[1])?;
        decoded.push((high << 4) | low);
    }
    Some(decoded)
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
