use std::{
    collections::{BTreeMap, VecDeque},
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{mpsc, Arc, Mutex, MutexGuard},
};

use serde::de::IgnoredAny;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ulid::Ulid;

use crate::session::runtime::ports::{
    hash_field, require_text, AppendResult, IntegrityDigest, OutstandingWaitTimer, RehydrateError,
    SessionAppendResult, SessionCreate, SessionCreateResult, SessionListCursor, SessionListItem,
    SessionListPage, SessionRef, StateDigestComponents, StoreError, StorePort,
    VerifiedSessionState, MAX_SESSION_LIST_LIMIT,
};
use crate::session::state::{
    DomainError, EventDraft, EventRecord, SessionEvent, SessionState, StreamVersion,
    EVENT_SCHEMA_VERSION, REDUCER_SCHEMA_VERSION, SESSION_CREATED_SCHEMA_VERSION,
    STATE_SCHEMA_VERSION,
};

const STATE_DIGEST_VERSION: i64 = 7;
const SESSION_SEGMENT_TARGET_BYTES: u64 = 16 * 1024 * 1024;
const HISTORICAL_SEGMENT_ZSTD_LEVEL: i32 = 12;
const SEGMENTS_DIRECTORY: &str = "segments";

pub struct JsonlEventStore {
    root: PathBuf,
    inner: Mutex<Inner>,
    session_locks: Mutex<BTreeMap<String, Arc<Mutex<()>>>>,
    create_lock: Mutex<()>,
    segment_target_bytes: u64,
    compressor: SegmentCompressor,
}

struct SegmentCompressor {
    sender: mpsc::Sender<PathBuf>,
}

struct Inner {
    sessions: BTreeMap<String, SessionLog>,
}

struct SessionLog {
    created_at_ms: i64,
    verified: VerifiedSessionState,
    segments: Vec<SegmentDescriptor>,
    current_bytes: u64,
    last_event_id: Ulid,
    last_domain_event_id: Ulid,
    deleted: bool,
}

#[derive(Clone, Debug)]
struct SegmentDescriptor {
    id: Ulid,
    plain: Option<PathBuf>,
    compressed: Option<PathBuf>,
}

impl SegmentDescriptor {
    fn read_path(&self) -> Result<(PathBuf, bool), StoreError> {
        if let Some(path) = self.compressed.as_ref() {
            if path.exists() {
                return Ok((path.clone(), true));
            }
        }
        if let Some(plain) = self.plain.as_ref() {
            let compressed = compressed_segment_path(plain);
            if compressed.exists() {
                return Ok((compressed, true));
            }
            if plain.exists() {
                return Ok((plain.clone(), false));
            }
        }
        Err(StoreError::Backend)
    }

    fn current_path(&self) -> Result<&Path, StoreError> {
        if self.compressed.is_some() {
            return Err(StoreError::Backend);
        }
        self.plain.as_deref().ok_or(StoreError::Backend)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum JsonlEvent {
    Domain {
        event_id: String,
        stream_id: String,
        stream_version: StreamVersion,
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
        through_stream_version: StreamVersion,
        state_schema_version: u32,
        reducer_schema_version: u32,
        state: Box<SessionState>,
    },
}

impl JsonlEvent {
    fn event_id(&self) -> &str {
        match self {
            Self::Domain { event_id, .. } | Self::Snapshot { event_id, .. } => event_id,
        }
    }

    fn domain_record(&self) -> Option<EventRecord> {
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
        Some(EventRecord {
            stream_id: stream_id.clone(),
            stream_version: *stream_version,
            event_id: event_id.clone(),
            event_schema_version: *event_schema_version,
            batch_index: *batch_index,
            batch_size: *batch_size,
            event: event.as_ref().clone(),
        })
    }

    fn batch_position(&self) -> (u32, u32) {
        match self {
            Self::Domain {
                batch_index,
                batch_size,
                ..
            }
            | Self::Snapshot {
                batch_index,
                batch_size,
                ..
            } => (*batch_index, *batch_size),
        }
    }
}

struct CurrentSegmentMetadata {
    complete_bytes: u64,
    first_event_id: Option<Ulid>,
    last_event_id: Option<Ulid>,
    last_domain_event_id: Option<Ulid>,
    latest_snapshot: Option<SnapshotLocation>,
    #[cfg_attr(not(test), allow(dead_code))]
    max_line_bytes: u64,
}

struct SnapshotLocation {
    line_start: u64,
    line_end: u64,
    preceding_domain_event_id: Option<String>,
    preceding_domain_stream_version: Option<StreamVersion>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum JsonlEventMetadata {
    Domain {
        event_id: String,
        stream_id: String,
        stream_version: StreamVersion,
        event_schema_version: u32,
        batch_index: u32,
        batch_size: u32,
        #[serde(rename = "event")]
        _event: IgnoredAny,
    },
    Snapshot {
        event_id: String,
        stream_id: String,
        batch_index: u32,
        batch_size: u32,
        through_event_id: String,
        through_stream_version: StreamVersion,
        state_schema_version: u32,
        reducer_schema_version: u32,
        #[serde(rename = "state")]
        _state: IgnoredAny,
    },
}

impl JsonlEventMetadata {
    fn event_id(&self) -> &str {
        match self {
            Self::Domain { event_id, .. } | Self::Snapshot { event_id, .. } => event_id,
        }
    }

    fn batch_position(&self) -> (u32, u32) {
        match self {
            Self::Domain {
                batch_index,
                batch_size,
                ..
            }
            | Self::Snapshot {
                batch_index,
                batch_size,
                ..
            } => (*batch_index, *batch_size),
        }
    }
}

struct MetadataRow {
    line_start: u64,
    line_end: u64,
    event: JsonlEventMetadata,
}

#[derive(Debug)]
struct RehydratedState {
    state: SessionState,
    prefix_digest: Vec<u8>,
    digest_components: StateDigestComponents,
}

impl SegmentCompressor {
    fn start() -> Result<Self, StoreError> {
        let (sender, receiver) = mpsc::channel::<PathBuf>();
        std::thread::Builder::new()
            .name("zork-session-compressor".to_owned())
            .spawn(move || {
                while let Ok(path) = receiver.recv() {
                    if compress_historical_segment(&path).is_err() {
                        tracing::warn!(
                            segment = %path.display(),
                            "failed to compress historical session segment"
                        );
                    }
                }
            })
            .map_err(|_| StoreError::Backend)?;
        Ok(Self { sender })
    }

    fn enqueue(&self, path: PathBuf) {
        if self.sender.send(path.clone()).is_err() {
            tracing::warn!(
                segment = %path.display(),
                "session segment compressor is unavailable"
            );
        }
    }
}

impl std::fmt::Debug for JsonlEventStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("JsonlEventStore")
            .finish_non_exhaustive()
    }
}

impl JsonlEventStore {
    pub fn open(root: impl AsRef<Path>) -> Result<Self, StoreError> {
        Self::open_with_segment_target(root, SESSION_SEGMENT_TARGET_BYTES)
    }

    fn open_with_segment_target(
        root: impl AsRef<Path>,
        segment_target_bytes: u64,
    ) -> Result<Self, StoreError> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root).map_err(|_| StoreError::Backend)?;
        let compressor = SegmentCompressor::start()?;
        let mut sessions = BTreeMap::new();
        let mut historical_plain = Vec::new();
        for entry in fs::read_dir(&root).map_err(|_| StoreError::Backend)? {
            let entry = entry.map_err(|_| StoreError::Backend)?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Some(session_id) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !path.join(SEGMENTS_DIRECTORY).is_dir() {
                continue;
            }
            if let Some((log, sealed_plain)) = load_session(&root, session_id)? {
                observe_canonical_ulid(session_id)?;
                crate::ids::observe_ulid(log.last_event_id);
                historical_plain.extend(sealed_plain);
                sessions.insert(session_id.to_owned(), log);
            }
        }
        for path in historical_plain {
            compressor.enqueue(path);
        }
        Ok(Self {
            root,
            inner: Mutex::new(Inner { sessions }),
            session_locks: Mutex::new(BTreeMap::new()),
            create_lock: Mutex::new(()),
            segment_target_bytes,
            compressor,
        })
    }

    pub fn delete_session(&self, session_id: &str) -> Result<(), StoreError> {
        require_text("session_id", session_id)?;
        let session_lock = self.session_lock(session_id)?;
        let _session_guard = session_lock.lock().map_err(|_| StoreError::Poisoned)?;
        let mut inner = self.lock()?;
        let log = require_session_mut(&mut inner, session_id)?;
        log.deleted = true;
        drop(inner);

        let directory = session_dir(&self.root, session_id);
        if let Err(error) = fs::remove_dir_all(&directory) {
            if error.kind() != std::io::ErrorKind::NotFound {
                if let Ok(mut inner) = self.lock() {
                    if let Some(log) = inner.sessions.get_mut(session_id) {
                        log.deleted = false;
                    }
                }
                return Err(StoreError::Backend);
            }
        }
        sync_directory(&self.root)?;
        self.lock()?.sessions.remove(session_id);
        Ok(())
    }

    fn lock(&self) -> Result<MutexGuard<'_, Inner>, StoreError> {
        self.inner.lock().map_err(|_| StoreError::Poisoned)
    }

    fn session_lock(&self, session_id: &str) -> Result<Arc<Mutex<()>>, StoreError> {
        let mut locks = self
            .session_locks
            .lock()
            .map_err(|_| StoreError::Poisoned)?;
        Ok(locks
            .entry(session_id.to_owned())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone())
    }
}

fn session_dir(root: &Path, session_id: &str) -> PathBuf {
    root.join(session_id)
}

fn segments_dir(root: &Path, session_id: &str) -> PathBuf {
    session_dir(root, session_id).join(SEGMENTS_DIRECTORY)
}

fn segment_path(root: &Path, session_id: &str, first_event_id: &str) -> PathBuf {
    segments_dir(root, session_id).join(format!("{first_event_id}.jsonl"))
}

fn compressed_segment_path(path: &Path) -> PathBuf {
    path.with_extension("jsonl.zst")
}

fn discover_segments(root: &Path, session_id: &str) -> Result<Vec<SegmentDescriptor>, StoreError> {
    let mut discovered = BTreeMap::<Ulid, SegmentDescriptor>::new();
    for entry in fs::read_dir(segments_dir(root, session_id)).map_err(|_| StoreError::Backend)? {
        let entry = entry.map_err(|_| StoreError::Backend)?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| StoreError::Backend)?;
        if name.starts_with('.') && name.ends_with(".tmp") {
            continue;
        }
        let (id_text, compressed) = if let Some(id) = name.strip_suffix(".jsonl.zst") {
            (id, true)
        } else if let Some(id) = name.strip_suffix(".jsonl") {
            (id, false)
        } else {
            return Err(StoreError::Backend);
        };
        let id = parse_canonical_ulid(id_text)?;
        let descriptor = discovered.entry(id).or_insert_with(|| SegmentDescriptor {
            id,
            plain: None,
            compressed: None,
        });
        let slot = if compressed {
            &mut descriptor.compressed
        } else {
            &mut descriptor.plain
        };
        if slot.replace(entry.path()).is_some() {
            return Err(StoreError::Backend);
        }
    }
    Ok(discovered.into_values().collect())
}

fn load_session(
    root: &Path,
    session_id: &str,
) -> Result<Option<(SessionLog, Vec<PathBuf>)>, StoreError> {
    let mut segments = discover_segments(root, session_id)?;
    let metadata = loop {
        let Some(current) = segments.last() else {
            return Ok(None);
        };
        if current.compressed.is_some() || current.plain.is_none() {
            return Err(StoreError::Backend);
        }
        let path = current.current_path()?.to_path_buf();
        let metadata = scan_current_segment_metadata(&path)?;
        if metadata.complete_bytes != 0 {
            break metadata;
        }
        fs::remove_file(&path).map_err(|_| StoreError::Backend)?;
        sync_directory(&segments_dir(root, session_id))?;
        segments.pop();
    };

    let current = segments.last().ok_or(StoreError::Backend)?;
    if metadata.first_event_id != Some(current.id) {
        return Err(StoreError::InvalidSessionStream);
    }
    if segments.len() > 1 && metadata.latest_snapshot.is_none() {
        return Err(StoreError::InvalidSessionStream);
    }

    let current_path = current.current_path()?;
    let (mut rehydrated, suffix_offset, had_snapshot) =
        if let Some(location) = metadata.latest_snapshot.as_ref() {
            let snapshot = read_jsonl_event_at(current_path, location.line_start)?;
            let preceding = location
                .preceding_domain_event_id
                .as_deref()
                .zip(location.preceding_domain_stream_version);
            let (state, _, snapshot_event_id) = snapshot_state(session_id, &snapshot, preceding)?;
            (
                RehydratedState {
                    digest_components: StateDigestComponents::from_state(&state)?,
                    prefix_digest: snapshot_prefix_seed(
                        session_id,
                        &snapshot_event_id,
                        state.stream_version,
                    ),
                    state,
                },
                location.line_end,
                true,
            )
        } else {
            if segments.len() != 1 {
                return Err(StoreError::InvalidSessionStream);
            }
            (empty_rehydrated(session_id), 0, false)
        };

    let mut first_domain = None;
    visit_jsonl_events_from(current_path, suffix_offset, |event| {
        let JsonlEvent::Domain { .. } = event else {
            return Err(StoreError::InvalidSessionStream);
        };
        let record = event.domain_record().ok_or(StoreError::Backend)?;
        if first_domain.is_none() {
            first_domain = Some(record.event.clone());
        }
        apply_domain_record(&mut rehydrated, &record)?;
        Ok(())
    })?;
    if !had_snapshot && !matches!(first_domain, Some(SessionEvent::SessionCreated { .. })) {
        return Err(DomainError::SessionNotCreated.into());
    }
    rehydrated.state.validate()?;
    let created_at_ms = rehydrated
        .state
        .created_at_ms
        .ok_or(StoreError::InvalidSessionStream)?;
    let last_event_id = metadata
        .last_event_id
        .ok_or(StoreError::InvalidSessionStream)?;
    let last_domain_event_id = metadata
        .last_domain_event_id
        .ok_or(StoreError::InvalidSessionStream)?;
    let verified = verified_state(rehydrated)?;
    let historical_plain = segments[..segments.len() - 1]
        .iter()
        .filter_map(|segment| segment.plain.clone())
        .collect();
    let deleted = session_dir(root, session_id).join("deleted").exists();
    Ok(Some((
        SessionLog {
            created_at_ms,
            verified,
            segments,
            current_bytes: metadata.complete_bytes,
            last_event_id,
            last_domain_event_id,
            deleted,
        },
        historical_plain,
    )))
}

fn snapshot_state(
    session_id: &str,
    event: &JsonlEvent,
    preceding_domain: Option<(&str, StreamVersion)>,
) -> Result<(SessionState, Ulid, String), StoreError> {
    let JsonlEvent::Snapshot {
        event_id,
        stream_id,
        through_event_id,
        through_stream_version,
        state_schema_version,
        reducer_schema_version,
        state,
        ..
    } = event
    else {
        return Err(StoreError::Backend);
    };
    let snapshot_id = parse_canonical_ulid(event_id)?;
    let through_id = parse_canonical_ulid(through_event_id)?;
    if stream_id != session_id
        || state.session_id != session_id
        || state.stream_version != *through_stream_version
        || *state_schema_version != STATE_SCHEMA_VERSION
        || *reducer_schema_version != REDUCER_SCHEMA_VERSION
        || through_id >= snapshot_id
    {
        return Err(StoreError::SnapshotStateMismatch);
    }
    if let Some((previous_event_id, previous_stream_version)) = preceding_domain {
        if previous_event_id != through_event_id
            || previous_stream_version != *through_stream_version
        {
            return Err(StoreError::SnapshotStateMismatch);
        }
    }
    state.validate()?;
    Ok((state.as_ref().clone(), through_id, event_id.clone()))
}

fn scan_current_segment_metadata(path: &Path) -> Result<CurrentSegmentMetadata, StoreError> {
    harden_private_file(path)?;
    let file = File::open(path).map_err(|_| StoreError::Backend)?;
    let physical_bytes = file.metadata().map_err(|_| StoreError::Backend)?.len();
    let mut reader = BufReader::new(file);
    let mut line = Vec::new();
    let mut batch = Vec::<MetadataRow>::new();
    let mut offset = 0_u64;
    let mut complete_bytes = 0_u64;
    let mut first_event_id = None;
    let mut last_event_id = None;
    let mut expected_stream_id = None::<String>;
    let mut last_domain_event_id = None;
    let mut last_domain_event_text = None;
    let mut last_domain_stream_version = None;
    let mut latest_snapshot = None;
    let mut max_line_bytes = 0_u64;

    loop {
        line.clear();
        let line_start = offset;
        let read = reader
            .read_until(b'\n', &mut line)
            .map_err(|_| StoreError::Backend)?;
        if read == 0 {
            break;
        }
        let read = u64::try_from(read).map_err(|_| StoreError::Backend)?;
        offset = offset.checked_add(read).ok_or(StoreError::Backend)?;
        if !line.ends_with(b"\n") {
            break;
        }
        max_line_bytes = max_line_bytes.max(read);
        let payload = line.strip_suffix(b"\n").ok_or(StoreError::Backend)?;
        if payload.is_empty() {
            return Err(StoreError::Backend);
        }
        let event = serde_json::from_slice::<JsonlEventMetadata>(payload)?;
        let (batch_index, batch_size) = event.batch_position();
        if batch.is_empty() {
            if batch_index != 0 || batch_size == 0 {
                return Err(StoreError::InvalidSessionStream);
            }
        } else {
            let (_, expected_size) = batch[0].event.batch_position();
            if batch_size != expected_size || usize::try_from(batch_index).ok() != Some(batch.len())
            {
                return Err(StoreError::InvalidSessionStream);
            }
        }
        batch.push(MetadataRow {
            line_start,
            line_end: offset,
            event,
        });
        let expected_size = usize::try_from(batch_size).map_err(|_| StoreError::Backend)?;
        if batch.len() < expected_size {
            continue;
        }
        if batch.len() != expected_size {
            return Err(StoreError::InvalidSessionStream);
        }

        for row in batch.drain(..) {
            let event_ulid = parse_canonical_ulid(row.event.event_id())?;
            if last_event_id.is_some_and(|previous| event_ulid <= previous) {
                return Err(StoreError::InvalidSessionStream);
            }
            first_event_id.get_or_insert(event_ulid);
            last_event_id = Some(event_ulid);
            match row.event {
                JsonlEventMetadata::Domain {
                    event_id,
                    stream_id,
                    stream_version,
                    event_schema_version,
                    batch_index: _,
                    batch_size: _,
                    _event: _,
                } => {
                    if expected_stream_id.get_or_insert_with(|| stream_id.clone()) != &stream_id
                        || event_schema_version != EVENT_SCHEMA_VERSION
                    {
                        return Err(StoreError::InvalidSessionStream);
                    }
                    last_domain_event_id = Some(event_ulid);
                    last_domain_event_text = Some(event_id);
                    last_domain_stream_version = Some(stream_version);
                }
                JsonlEventMetadata::Snapshot {
                    event_id: _,
                    stream_id,
                    batch_index: _,
                    batch_size: _,
                    through_event_id,
                    through_stream_version,
                    state_schema_version,
                    reducer_schema_version,
                    _state: _,
                } => {
                    if expected_stream_id.get_or_insert_with(|| stream_id.clone()) != &stream_id
                        || last_domain_event_text.as_deref() != Some(through_event_id.as_str())
                        || last_domain_stream_version != Some(through_stream_version)
                        || state_schema_version != STATE_SCHEMA_VERSION
                        || reducer_schema_version != REDUCER_SCHEMA_VERSION
                    {
                        return Err(StoreError::SnapshotStateMismatch);
                    }
                    latest_snapshot = Some(SnapshotLocation {
                        line_start: row.line_start,
                        line_end: row.line_end,
                        preceding_domain_event_id: last_domain_event_text.clone(),
                        preceding_domain_stream_version: last_domain_stream_version,
                    });
                }
            }
        }
        complete_bytes = offset;
    }

    if complete_bytes != physical_bytes {
        let file = OpenOptions::new()
            .write(true)
            .open(path)
            .map_err(|_| StoreError::Backend)?;
        file.set_len(complete_bytes)
            .map_err(|_| StoreError::Backend)?;
        file.sync_all().map_err(|_| StoreError::Backend)?;
        sync_directory(path.parent().ok_or(StoreError::Backend)?)?;
    }

    Ok(CurrentSegmentMetadata {
        complete_bytes,
        first_event_id,
        last_event_id,
        last_domain_event_id,
        latest_snapshot,
        max_line_bytes,
    })
}

fn read_jsonl_event_at(path: &Path, offset: u64) -> Result<JsonlEvent, StoreError> {
    harden_private_file(path)?;
    let mut file = File::open(path).map_err(|_| StoreError::Backend)?;
    file.seek(SeekFrom::Start(offset))
        .map_err(|_| StoreError::Backend)?;
    let mut reader = BufReader::new(file);
    let mut line = Vec::new();
    if reader
        .read_until(b'\n', &mut line)
        .map_err(|_| StoreError::Backend)?
        == 0
        || !line.ends_with(b"\n")
    {
        return Err(StoreError::Backend);
    }
    let payload = line.strip_suffix(b"\n").ok_or(StoreError::Backend)?;
    serde_json::from_slice(payload).map_err(StoreError::from)
}

fn visit_jsonl_events_from(
    path: &Path,
    offset: u64,
    mut visit: impl FnMut(JsonlEvent) -> Result<(), StoreError>,
) -> Result<(), StoreError> {
    harden_private_file(path)?;
    let mut file = File::open(path).map_err(|_| StoreError::Backend)?;
    file.seek(SeekFrom::Start(offset))
        .map_err(|_| StoreError::Backend)?;
    let mut reader = BufReader::new(file);
    let mut line = Vec::new();
    loop {
        line.clear();
        let read = reader
            .read_until(b'\n', &mut line)
            .map_err(|_| StoreError::Backend)?;
        if read == 0 {
            return Ok(());
        }
        if !line.ends_with(b"\n") {
            return Err(StoreError::Backend);
        }
        let payload = line.strip_suffix(b"\n").ok_or(StoreError::Backend)?;
        visit(serde_json::from_slice(payload)?)?;
    }
}

fn visit_descriptor_events(
    descriptor: &SegmentDescriptor,
    is_current: bool,
    mut visit: impl FnMut(JsonlEvent) -> Result<(), StoreError>,
) -> Result<(), StoreError> {
    if is_current {
        let path = descriptor.current_path()?;
        scan_current_segment_metadata(path)?;
        return visit_complete_segment_file(path, false, &mut visit);
    }
    for _ in 0..2 {
        let (path, compressed) = descriptor.read_path()?;
        match visit_complete_segment_file(&path, compressed, &mut visit) {
            Ok(()) => return Ok(()),
            Err(_) if !path.exists() => continue,
            Err(error) => return Err(error),
        }
    }
    Err(StoreError::Backend)
}

fn visit_complete_segment_file(
    path: &Path,
    compressed: bool,
    visit: &mut impl FnMut(JsonlEvent) -> Result<(), StoreError>,
) -> Result<(), StoreError> {
    harden_private_file(path)?;
    let file = File::open(path).map_err(|_| StoreError::Backend)?;
    let input: Box<dyn Read> = if compressed {
        Box::new(zstd::stream::read::Decoder::new(file).map_err(|_| StoreError::Backend)?)
    } else {
        Box::new(file)
    };
    let mut reader = BufReader::new(input);
    let mut line = Vec::new();
    let mut event_count = 0_u64;
    let mut next_batch_index = 0_u32;
    let mut current_batch_size = 0_u32;
    loop {
        line.clear();
        let read = reader
            .read_until(b'\n', &mut line)
            .map_err(|_| StoreError::Backend)?;
        if read == 0 {
            break;
        }
        if !line.ends_with(b"\n") {
            return Err(StoreError::Backend);
        }
        let payload = line.strip_suffix(b"\n").ok_or(StoreError::Backend)?;
        if payload.is_empty() {
            return Err(StoreError::Backend);
        }
        let event = serde_json::from_slice::<JsonlEvent>(payload)?;
        let (batch_index, batch_size) = event.batch_position();
        if next_batch_index == 0 {
            if batch_index != 0 || batch_size == 0 {
                return Err(StoreError::InvalidSessionStream);
            }
            current_batch_size = batch_size;
        } else if batch_index != next_batch_index || batch_size != current_batch_size {
            return Err(StoreError::InvalidSessionStream);
        }
        next_batch_index = next_batch_index.checked_add(1).ok_or(StoreError::Backend)?;
        if next_batch_index == current_batch_size {
            next_batch_index = 0;
            current_batch_size = 0;
        }
        event_count = event_count.checked_add(1).ok_or(StoreError::Backend)?;
        visit(event)?;
    }
    if event_count == 0 || next_batch_index != 0 {
        return Err(StoreError::Backend);
    }
    Ok(())
}

fn visit_history_segment(
    stream_id: &str,
    descriptor: &SegmentDescriptor,
    is_current: bool,
    upper_event_id: Option<Ulid>,
    mut visit: impl FnMut(JsonlEvent) -> Result<(), StoreError>,
) -> Result<(), StoreError> {
    let mut first = true;
    let mut previous_event_id = None;
    let mut previous_domain_version: Option<StreamVersion> = None;
    let mut last_domain = None::<(String, StreamVersion)>;
    visit_descriptor_events(descriptor, is_current, |event| {
        let event_id = parse_canonical_ulid(event.event_id())?;
        if first {
            if event_id != descriptor.id {
                return Err(StoreError::InvalidSessionStream);
            }
            first = false;
        }
        if previous_event_id.is_some_and(|previous| event_id <= previous)
            || upper_event_id.is_some_and(|upper| event_id >= upper)
        {
            return Err(StoreError::InvalidSessionStream);
        }
        previous_event_id = Some(event_id);
        match &event {
            JsonlEvent::Domain { .. } => {
                let record = event.domain_record().ok_or(StoreError::Backend)?;
                if record.stream_id != stream_id
                    || record.event_schema_version != EVENT_SCHEMA_VERSION
                    || previous_domain_version.is_some_and(|version| {
                        version.checked_add(1) != Some(record.stream_version)
                    })
                {
                    return Err(StoreError::InvalidSessionStream);
                }
                record.event.validate()?;
                validate_created_object_identity(&record.event_id, &record.event)?;
                previous_domain_version = Some(record.stream_version);
                last_domain = Some((record.event_id, record.stream_version));
            }
            JsonlEvent::Snapshot {
                stream_id: stored_stream_id,
                through_event_id,
                through_stream_version,
                state_schema_version,
                reducer_schema_version,
                state,
                ..
            } => {
                if stored_stream_id != stream_id
                    || last_domain.as_ref()
                        != Some(&(through_event_id.clone(), *through_stream_version))
                    || *state_schema_version != STATE_SCHEMA_VERSION
                    || *reducer_schema_version != REDUCER_SCHEMA_VERSION
                    || state.session_id != stream_id
                    || state.stream_version != *through_stream_version
                {
                    return Err(StoreError::SnapshotStateMismatch);
                }
                state.validate()?;
            }
        }
        visit(event)
    })?;
    if first {
        return Err(StoreError::InvalidSessionStream);
    }
    Ok(())
}

fn sync_directory(path: &Path) -> Result<(), StoreError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| StoreError::Backend)
}

fn harden_private_file(path: &Path) -> Result<(), StoreError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| StoreError::Backend)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(StoreError::Backend);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o777 != 0o600 {
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))
                .map_err(|_| StoreError::Backend)?;
        }
    }
    Ok(())
}

fn open_event_file(path: &Path, create_new: bool) -> Result<File, StoreError> {
    let mut options = OpenOptions::new();
    options.append(true);
    if create_new {
        options.create_new(true);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    options.open(path).map_err(|_| StoreError::Backend)
}

fn serialize_jsonl(events: &[JsonlEvent]) -> Result<Vec<u8>, StoreError> {
    let mut bytes = Vec::new();
    for event in events {
        serde_json::to_writer(&mut bytes, event)?;
        bytes.push(b'\n');
    }
    Ok(bytes)
}

fn append_bytes(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    harden_private_file(path)?;
    let mut file = open_event_file(path, false)?;
    file.write_all(bytes).map_err(|_| StoreError::Backend)?;
    file.sync_all().map_err(|_| StoreError::Backend)
}

fn create_segment(
    root: &Path,
    session_id: &str,
    first_event_id: &str,
    bytes: &[u8],
) -> Result<PathBuf, StoreError> {
    let segment_directory = segments_dir(root, session_id);
    fs::create_dir_all(&segment_directory).map_err(|_| StoreError::Backend)?;
    let path = segment_path(root, session_id, first_event_id);
    let mut file = open_event_file(&path, true)?;
    file.write_all(bytes).map_err(|_| StoreError::Backend)?;
    file.sync_all().map_err(|_| StoreError::Backend)?;
    sync_directory(&segment_directory)?;
    Ok(path)
}

fn compress_historical_segment(path: &Path) -> Result<(), StoreError> {
    if !path.exists() {
        return Ok(());
    }
    harden_private_file(path)?;
    let compressed = compressed_segment_path(path);
    let parent = path.parent().ok_or(StoreError::Backend)?;
    if compressed.exists() {
        harden_private_file(&compressed)?;
        fs::remove_file(path).map_err(|_| StoreError::Backend)?;
        return sync_directory(parent);
    }

    let compressed_name = compressed
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(StoreError::Backend)?;
    let temporary = parent.join(format!(".{compressed_name}.{}.tmp", Ulid::new()));
    let result = (|| {
        let mut source = File::open(path).map_err(|_| StoreError::Backend)?;
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let destination = options.open(&temporary).map_err(|_| StoreError::Backend)?;
        let mut encoder =
            zstd::stream::write::Encoder::new(destination, HISTORICAL_SEGMENT_ZSTD_LEVEL)
                .map_err(|_| StoreError::Backend)?;
        std::io::copy(&mut source, &mut encoder).map_err(|_| StoreError::Backend)?;
        let destination = encoder.finish().map_err(|_| StoreError::Backend)?;
        destination.sync_all().map_err(|_| StoreError::Backend)?;
        drop(destination);
        fs::rename(&temporary, &compressed).map_err(|_| StoreError::Backend)?;
        sync_directory(parent)?;
        fs::remove_file(path).map_err(|_| StoreError::Backend)?;
        sync_directory(parent)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn require_session<'a>(inner: &'a Inner, stream_id: &str) -> Result<&'a SessionLog, StoreError> {
    let log = inner
        .sessions
        .get(stream_id)
        .ok_or(StoreError::SessionNotFound)?;
    if log.deleted {
        return Err(StoreError::SessionNotFound);
    }
    Ok(log)
}

fn require_session_mut<'a>(
    inner: &'a mut Inner,
    stream_id: &str,
) -> Result<&'a mut SessionLog, StoreError> {
    let log = inner
        .sessions
        .get_mut(stream_id)
        .ok_or(StoreError::SessionNotFound)?;
    if log.deleted {
        return Err(StoreError::SessionNotFound);
    }
    Ok(log)
}

fn session_ref(session_id: &str) -> SessionRef {
    SessionRef {
        session_id: session_id.to_owned(),
    }
}

fn validate_event_drafts(events: &[EventDraft]) -> Result<(), StoreError> {
    if events.is_empty() {
        return Err(StoreError::EmptyEventBatch);
    }
    u32::try_from(events.len()).map_err(|_| StoreError::IntegerRange {
        field: "event batch length",
    })?;
    let mut previous = None;
    for event in events {
        let event_id = parse_canonical_ulid(&event.event_id)?;
        if previous.is_some_and(|previous| event_id <= previous) {
            return Err(StoreError::InvalidSessionStream);
        }
        previous = Some(event_id);
        event.event.validate()?;
        validate_created_object_identity(&event.event_id, &event.event)?;
    }
    Ok(())
}

fn validate_created_object_identity(
    event_id: &str,
    event: &SessionEvent,
) -> Result<(), StoreError> {
    if event
        .created_object_id()
        .is_some_and(|object_id| object_id != event_id)
    {
        return Err(StoreError::InvalidSessionStream);
    }
    Ok(())
}

fn stored_batch(
    log: &SessionLog,
    first_event_id: Ulid,
) -> Result<Option<Vec<EventRecord>>, StoreError> {
    let insertion = log
        .segments
        .partition_point(|segment| segment.id <= first_event_id);
    let Some(index) = insertion.checked_sub(1) else {
        return Ok(None);
    };
    let descriptor = &log.segments[index];
    let target = first_event_id.to_string();
    let mut first = true;
    let mut collecting = false;
    let mut expected_size = 0_usize;
    let mut collected_rows = 0_usize;
    let mut records = None::<Vec<EventRecord>>;
    visit_descriptor_events(descriptor, index + 1 == log.segments.len(), |event| {
        if first {
            if parse_canonical_ulid(event.event_id())? != descriptor.id {
                return Err(StoreError::InvalidSessionStream);
            }
            first = false;
        }
        if event.event_id() == target {
            let Some(record) = event.domain_record() else {
                return Ok(());
            };
            if record.batch_index != 0 || record.batch_size == 0 {
                return Err(StoreError::InvalidSessionStream);
            }
            expected_size = usize::try_from(record.batch_size).map_err(|_| StoreError::Backend)?;
            records = Some(Vec::with_capacity(expected_size));
            collecting = true;
        }
        if collecting {
            collected_rows = collected_rows.checked_add(1).ok_or(StoreError::Backend)?;
            if let Some(record) = event.domain_record() {
                records.as_mut().ok_or(StoreError::Backend)?.push(record);
            } else if collected_rows != expected_size {
                return Err(StoreError::InvalidSessionStream);
            }
            if collected_rows == expected_size {
                collecting = false;
            }
        }
        Ok(())
    })?;
    if collecting {
        return Err(StoreError::InvalidSessionStream);
    }
    Ok(records)
}

fn replay_if_stored(
    stream_id: &str,
    log: &SessionLog,
    events: &[EventDraft],
) -> Result<Option<SessionAppendResult>, StoreError> {
    let first = events.first().ok_or(StoreError::EmptyEventBatch)?;
    let first_id = parse_canonical_ulid(&first.event_id)?;
    if first_id > log.last_event_id {
        return Ok(None);
    }
    let Some(stored) = stored_batch(log, first_id)? else {
        return Err(StoreError::InvalidSessionStream);
    };
    if stored.len() != events.len()
        || !stored.iter().zip(events).all(|(stored, submitted)| {
            stored.event_id == submitted.event_id && stored.event == submitted.event
        })
    {
        return Err(StoreError::EventBatchConflict {
            event_id: first.event_id.clone(),
        });
    }
    let stream_version = stored
        .last()
        .map(|record| record.stream_version)
        .ok_or(StoreError::EmptyEventBatch)?;
    Ok(Some(SessionAppendResult {
        append: AppendResult {
            stream_id: stream_id.to_owned(),
            events: stored,
            stream_version,
            replayed: true,
        },
        state: log.verified.clone(),
    }))
}

fn apply_event_batch(
    stream_id: &str,
    expected_version: StreamVersion,
    mut rehydrated: RehydratedState,
    drafts: &[EventDraft],
    persisted_batch_size: u32,
) -> Result<(RehydratedState, Vec<EventRecord>, Vec<JsonlEvent>), StoreError> {
    let domain_size = u32::try_from(drafts.len()).map_err(|_| StoreError::IntegerRange {
        field: "event batch length",
    })?;
    if persisted_batch_size < domain_size {
        return Err(StoreError::InvalidSessionStream);
    }
    let mut records = Vec::with_capacity(drafts.len());
    let mut persisted = Vec::with_capacity(drafts.len());
    for (index, draft) in drafts.iter().enumerate() {
        let offset = u64::try_from(index + 1).map_err(|_| StoreError::IntegerRange {
            field: "event batch index",
        })?;
        let stream_version =
            expected_version
                .checked_add(offset)
                .ok_or(StoreError::IntegerRange {
                    field: "stream_version",
                })?;
        let batch_index = u32::try_from(index).map_err(|_| StoreError::IntegerRange {
            field: "event batch index",
        })?;
        let record = EventRecord {
            stream_id: stream_id.to_owned(),
            stream_version,
            event_id: draft.event_id.clone(),
            event_schema_version: EVENT_SCHEMA_VERSION,
            batch_index,
            batch_size: persisted_batch_size,
            event: draft.event.clone(),
        };
        apply_domain_record(&mut rehydrated, &record)?;
        persisted.push(JsonlEvent::Domain {
            event_id: record.event_id.clone(),
            stream_id: record.stream_id.clone(),
            stream_version: record.stream_version,
            event_schema_version: record.event_schema_version,
            batch_index,
            batch_size: persisted_batch_size,
            event: Box::new(record.event.clone()),
        });
        records.push(record);
    }
    Ok((rehydrated, records, persisted))
}

fn append_to_log(
    stream_id: &str,
    log: &mut SessionLog,
    expected_version: StreamVersion,
    preverified: RehydratedState,
    events: &[EventDraft],
) -> Result<SessionAppendResult, StoreError> {
    if let Some(replayed) = replay_if_stored(stream_id, log, events)? {
        return Ok(replayed);
    }
    let actual = log.verified.stream_version;
    if expected_version != actual {
        return Err(StoreError::OptimisticConcurrency {
            stream_id: stream_id.to_owned(),
            expected: expected_version,
            actual,
        });
    }
    let first_id =
        parse_canonical_ulid(&events.first().ok_or(StoreError::EmptyEventBatch)?.event_id)?;
    if first_id <= log.last_event_id || preverified.state.stream_version != expected_version {
        return Err(StoreError::InvalidSessionStream);
    }
    let batch_size = u32::try_from(events.len()).map_err(|_| StoreError::IntegerRange {
        field: "event batch length",
    })?;
    let (rehydrated, records, persisted) =
        apply_event_batch(stream_id, expected_version, preverified, events, batch_size)?;
    let bytes = serialize_jsonl(&persisted)?;
    let current = log
        .segments
        .last()
        .ok_or(StoreError::InvalidSessionStream)?
        .current_path()?;
    append_bytes(current, &bytes)?;

    log.current_bytes = log
        .current_bytes
        .checked_add(u64::try_from(bytes.len()).map_err(|_| StoreError::Backend)?)
        .ok_or(StoreError::Backend)?;
    log.last_event_id =
        parse_canonical_ulid(&records.last().ok_or(StoreError::EmptyEventBatch)?.event_id)?;
    log.last_domain_event_id = log.last_event_id;
    let verified = verified_state(rehydrated)?;
    let stream_version = verified.stream_version;
    log.verified = verified.clone();
    Ok(SessionAppendResult {
        append: AppendResult {
            stream_id: stream_id.to_owned(),
            events: records,
            stream_version,
            replayed: false,
        },
        state: verified,
    })
}

fn create_log(
    root: &Path,
    stream_id: &str,
    events: &[EventDraft],
) -> Result<(SessionLog, SessionAppendResult), StoreError> {
    validate_event_drafts(events)?;
    let batch_size = u32::try_from(events.len()).map_err(|_| StoreError::IntegerRange {
        field: "event batch length",
    })?;
    let (rehydrated, records, persisted) = apply_event_batch(
        stream_id,
        0,
        empty_rehydrated(stream_id),
        events,
        batch_size,
    )?;
    let first = records.first().ok_or(StoreError::EmptyEventBatch)?;
    let SessionEvent::SessionCreated { created_at_ms, .. } = &first.event else {
        return Err(DomainError::SessionNotCreated.into());
    };
    let created_at_ms = *created_at_ms;
    if records.len() != 1 {
        return Err(StoreError::InvalidSessionStream);
    }
    let bytes = serialize_jsonl(&persisted)?;
    let path = create_segment(root, stream_id, &first.event_id, &bytes)?;
    let id = parse_canonical_ulid(&first.event_id)?;
    let verified = verified_state(rehydrated)?;
    let result = SessionAppendResult {
        append: AppendResult {
            stream_id: stream_id.to_owned(),
            events: records,
            stream_version: verified.stream_version,
            replayed: false,
        },
        state: verified.clone(),
    };
    Ok((
        SessionLog {
            created_at_ms,
            verified,
            segments: vec![SegmentDescriptor {
                id,
                plain: Some(path),
                compressed: None,
            }],
            current_bytes: u64::try_from(bytes.len()).map_err(|_| StoreError::Backend)?,
            last_event_id: id,
            last_domain_event_id: id,
            deleted: false,
        },
        result,
    ))
}

fn carried_state(
    stream_id: &str,
    current: VerifiedSessionState,
    log: &SessionLog,
) -> Result<RehydratedState, StoreError> {
    if current.session_id != stream_id
        || current.created_at_ms != Some(log.created_at_ms)
        || current.stream_version != log.verified.stream_version
        || !digest_matches(&current.prefix_digest, &log.verified.prefix_digest)
        || current.state_digest_version != log.verified.state_digest_version
        || !digest_matches(&current.state_digest, &log.verified.state_digest)
    {
        return Err(StoreError::RehydrationIntegrity {
            stream_id: stream_id.to_owned(),
            version: current.stream_version,
        });
    }
    let VerifiedSessionState {
        state,
        prefix_digest,
        digest_components,
        ..
    } = current;
    Ok(RehydratedState {
        state: Arc::try_unwrap(state).unwrap_or_else(|state| (*state).clone()),
        prefix_digest,
        digest_components,
    })
}

fn current_state(
    stream_id: &str,
    current: &SessionState,
    log: &SessionLog,
) -> Result<RehydratedState, StoreError> {
    current.validate()?;
    if current.session_id != stream_id
        || current.created_at_ms != Some(log.created_at_ms)
        || current.stream_version != log.verified.stream_version
    {
        return Err(StoreError::InvalidSessionStream);
    }
    let digest_components = StateDigestComponents::from_state(current)?;
    let digest = state_digest_for_version(
        current,
        &digest_components,
        log.verified.state_digest_version,
    )?;
    if !digest_matches(&digest, &log.verified.state_digest) {
        return Err(StoreError::RehydrationIntegrity {
            stream_id: stream_id.to_owned(),
            version: current.stream_version,
        });
    }
    Ok(RehydratedState {
        state: current.clone(),
        prefix_digest: log.verified.prefix_digest.clone(),
        digest_components,
    })
}

impl StorePort for JsonlEventStore {
    fn create_session(&self, create: &SessionCreate) -> Result<SessionCreateResult, StoreError> {
        create.selection.validate()?;
        if create.created_at_ms < 0 {
            return Err(DomainError::InvalidCreatedAt.into());
        }
        let _create_guard = self.create_lock.lock().map_err(|_| StoreError::Poisoned)?;
        loop {
            let event = EventDraft::identified(|event_id| SessionEvent::SessionCreated {
                schema_version: SESSION_CREATED_SCHEMA_VERSION,
                session_id: event_id.to_owned(),
                created_at_ms: create.created_at_ms,
                selection: create.selection.clone(),
                system_prompt: create.system_prompt.clone(),
                workspace: create.workspace.clone(),
            });
            let session_id = event.event_id.clone();
            if self.lock()?.sessions.contains_key(&session_id)
                || session_dir(&self.root, &session_id).exists()
            {
                continue;
            }
            let session_lock = self.session_lock(&session_id)?;
            let _session_guard = session_lock.lock().map_err(|_| StoreError::Poisoned)?;
            let directory = session_dir(&self.root, &session_id);
            fs::create_dir_all(&directory).map_err(|_| StoreError::Backend)?;
            sync_directory(&self.root)?;
            let events = [event];
            let (log, appended) = create_log(&self.root, &session_id, &events)?;
            self.lock()?.sessions.insert(session_id.clone(), log);
            if appended.state.stream_version != 1 {
                return Err(StoreError::InvalidSessionStream);
            }
            return Ok(SessionCreateResult {
                append: appended.append,
                state: appended.state.into_state(),
            });
        }
    }

    fn append(
        &self,
        stream_id: &str,
        current: &SessionState,
        events: &[EventDraft],
    ) -> Result<SessionAppendResult, StoreError> {
        require_text("stream_id", stream_id)?;
        validate_event_drafts(events)?;
        let session_lock = self.session_lock(stream_id)?;
        let _session_guard = session_lock.lock().map_err(|_| StoreError::Poisoned)?;
        let mut inner = self.lock()?;
        let log = require_session_mut(&mut inner, stream_id)?;
        if let Some(replayed) = replay_if_stored(stream_id, log, events)? {
            return Ok(replayed);
        }
        if current.stream_version != log.verified.stream_version {
            return Err(StoreError::OptimisticConcurrency {
                stream_id: stream_id.to_owned(),
                expected: current.stream_version,
                actual: log.verified.stream_version,
            });
        }
        let preverified = current_state(stream_id, current, log)?;
        append_to_log(stream_id, log, current.stream_version, preverified, events)
    }

    fn append_verified(
        &self,
        stream_id: &str,
        current: VerifiedSessionState,
        events: &[EventDraft],
    ) -> Result<SessionAppendResult, StoreError> {
        require_text("stream_id", stream_id)?;
        validate_event_drafts(events)?;
        let expected_version = current.stream_version;
        let session_lock = self.session_lock(stream_id)?;
        let _session_guard = session_lock.lock().map_err(|_| StoreError::Poisoned)?;
        let mut inner = self.lock()?;
        let log = require_session_mut(&mut inner, stream_id)?;
        if let Some(replayed) = replay_if_stored(stream_id, log, events)? {
            return Ok(replayed);
        }
        if expected_version != log.verified.stream_version {
            return Err(StoreError::OptimisticConcurrency {
                stream_id: stream_id.to_owned(),
                expected: expected_version,
                actual: log.verified.stream_version,
            });
        }
        let preverified = carried_state(stream_id, current, log)?;
        append_to_log(stream_id, log, expected_version, preverified, events)
    }

    fn rehydrate(&self, stream_id: &str) -> Result<SessionState, RehydrateError> {
        Ok(self.rehydrate_verified(stream_id)?.into_state())
    }

    fn rehydrate_verified(&self, stream_id: &str) -> Result<VerifiedSessionState, RehydrateError> {
        require_text("stream_id", stream_id)?;
        let inner = self.lock()?;
        Ok(require_session(&inner, stream_id)?.verified.clone())
    }

    fn read_stream(
        &self,
        stream_id: &str,
        after_version: StreamVersion,
        limit: usize,
    ) -> Result<Vec<EventRecord>, StoreError> {
        require_text("stream_id", stream_id)?;
        if limit == 0 {
            return Ok(Vec::new());
        }
        let session_lock = self.session_lock(stream_id)?;
        let _session_guard = session_lock.lock().map_err(|_| StoreError::Poisoned)?;
        let inner = self.lock()?;
        let segments = require_session(&inner, stream_id)?.segments.clone();
        drop(inner);
        let mut records = Vec::new();
        let mut previous_event_id = None;
        let mut last_domain_event_id = None;
        let mut version = 0_u64;
        for (index, descriptor) in segments.iter().enumerate() {
            let mut first = true;
            let upper_event_id = segments.get(index + 1).map(|segment| segment.id);
            visit_descriptor_events(descriptor, index + 1 == segments.len(), |event| {
                let event_id = parse_canonical_ulid(event.event_id())?;
                if first {
                    if event_id != descriptor.id {
                        return Err(StoreError::InvalidSessionStream);
                    }
                    first = false;
                }
                if previous_event_id.is_some_and(|previous| event_id <= previous)
                    || upper_event_id.is_some_and(|upper| event_id >= upper)
                {
                    return Err(StoreError::InvalidSessionStream);
                }
                previous_event_id = Some(event_id);
                match event {
                    JsonlEvent::Domain { .. } => {
                        let record = event.domain_record().ok_or(StoreError::Backend)?;
                        version = version.checked_add(1).ok_or(StoreError::Backend)?;
                        if record.stream_id != stream_id
                            || record.stream_version != version
                            || record.event_schema_version != EVENT_SCHEMA_VERSION
                        {
                            return Err(StoreError::InvalidSessionStream);
                        }
                        record.event.validate()?;
                        last_domain_event_id = Some(record.event_id.clone());
                        if record.stream_version > after_version && records.len() < limit {
                            records.push(record);
                        }
                    }
                    JsonlEvent::Snapshot {
                        stream_id: stored_stream_id,
                        through_event_id,
                        through_stream_version,
                        state_schema_version,
                        reducer_schema_version,
                        state,
                        ..
                    } => {
                        if stored_stream_id != stream_id
                            || through_stream_version != version
                            || last_domain_event_id.as_deref() != Some(through_event_id.as_str())
                            || state_schema_version != STATE_SCHEMA_VERSION
                            || reducer_schema_version != REDUCER_SCHEMA_VERSION
                            || state.session_id != stream_id
                            || state.stream_version != version
                        {
                            return Err(StoreError::SnapshotStateMismatch);
                        }
                        state.validate()?;
                    }
                }
                Ok(())
            })?;
            if first {
                return Err(StoreError::InvalidSessionStream);
            }
            if records.len() == limit {
                return Ok(records);
            }
        }
        Ok(records)
    }

    fn read_stream_before(
        &self,
        stream_id: &str,
        before_event_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<EventRecord>, StoreError> {
        require_text("stream_id", stream_id)?;
        if limit == 0 {
            return Ok(Vec::new());
        }
        let session_lock = self.session_lock(stream_id)?;
        let _session_guard = session_lock.lock().map_err(|_| StoreError::Poisoned)?;
        let inner = self.lock()?;
        let segments = require_session(&inner, stream_id)?.segments.clone();
        drop(inner);
        let before = before_event_id.map(parse_lookup_ulid).transpose()?;
        let end = before.map_or(segments.len(), |before| {
            segments.partition_point(|segment| segment.id < before)
        });
        if end == 0 {
            return Ok(Vec::new());
        }

        let mut records = VecDeque::with_capacity(limit);
        for index in (0..end).rev() {
            let descriptor = &segments[index];
            let remaining = limit.saturating_sub(records.len());
            let mut local = VecDeque::with_capacity(remaining);
            visit_history_segment(
                stream_id,
                descriptor,
                index + 1 == segments.len(),
                segments.get(index + 1).map(|segment| segment.id),
                |event| {
                    let event_id = parse_canonical_ulid(event.event_id())?;
                    if before.is_some_and(|before| event_id >= before) {
                        return Ok(());
                    }
                    if let Some(record) = event.domain_record() {
                        local.push_back(record);
                        if local.len() > remaining {
                            local.pop_front();
                        }
                    }
                    Ok(())
                },
            )?;
            while let Some(record) = local.pop_back() {
                records.push_front(record);
            }
            if records.len() == limit {
                return Ok(records.into_iter().collect());
            }
        }
        Ok(records.into_iter().collect())
    }

    fn read_event(
        &self,
        stream_id: &str,
        event_id: &str,
    ) -> Result<Option<EventRecord>, StoreError> {
        require_text("stream_id", stream_id)?;
        let session_lock = self.session_lock(stream_id)?;
        let _session_guard = session_lock.lock().map_err(|_| StoreError::Poisoned)?;
        let inner = self.lock()?;
        let segments = require_session(&inner, stream_id)?.segments.clone();
        drop(inner);
        let target = parse_lookup_ulid(event_id)?;
        let end = segments.partition_point(|segment| segment.id <= target);
        let Some(index) = end.checked_sub(1) else {
            return Ok(None);
        };
        let descriptor = &segments[index];
        let mut found = None;
        visit_history_segment(
            stream_id,
            descriptor,
            index + 1 == segments.len(),
            segments.get(index + 1).map(|segment| segment.id),
            |event| {
                if event.event_id() == event_id {
                    found = event.domain_record();
                }
                Ok(())
            },
        )?;
        Ok(found)
    }

    fn list_outstanding_wait_timers(&self) -> Result<Vec<OutstandingWaitTimer>, StoreError> {
        let inner = self.lock()?;
        let mut timers = Vec::new();
        for (session_id, log) in &inner.sessions {
            if log.deleted {
                continue;
            }
            if let Some(timer) = &log.verified.active_timer {
                timers.push(OutstandingWaitTimer {
                    session_id: session_id.clone(),
                    wait_id: timer.wait_id.clone(),
                    deadline_ms: timer.deadline_ms,
                });
            }
        }
        timers.sort_by(|left, right| {
            left.deadline_ms
                .cmp(&right.deadline_ms)
                .then_with(|| left.session_id.cmp(&right.session_id))
        });
        Ok(timers)
    }

    fn list_runnable_sessions(&self) -> Result<Vec<SessionRef>, StoreError> {
        let inner = self.lock()?;
        let mut sessions = inner
            .sessions
            .iter()
            .filter(|(_, log)| !log.deleted && log.verified.is_startup_runnable())
            .map(|(session_id, _)| session_ref(session_id))
            .collect::<Vec<_>>();
        sessions.sort_by(|left, right| left.session_id.cmp(&right.session_id));
        Ok(sessions)
    }

    fn list_active_activations(&self) -> Result<Vec<SessionRef>, StoreError> {
        let inner = self.lock()?;
        let mut sessions = inner
            .sessions
            .iter()
            .filter(|(_, log)| !log.deleted && log.verified.active_activation.is_some())
            .map(|(session_id, _)| session_ref(session_id))
            .collect::<Vec<_>>();
        sessions.sort_by(|left, right| left.session_id.cmp(&right.session_id));
        Ok(sessions)
    }

    fn list_sessions(&self, limit: usize) -> Result<Vec<SessionListItem>, StoreError> {
        Ok(self.list_sessions_page(None, limit)?.items)
    }

    fn list_sessions_page(
        &self,
        cursor: Option<&SessionListCursor>,
        limit: usize,
    ) -> Result<SessionListPage, StoreError> {
        if !(1..=MAX_SESSION_LIST_LIMIT).contains(&limit) {
            return Err(StoreError::InvalidSessionListLimit);
        }
        let inner = self.lock()?;
        let mut items = inner
            .sessions
            .iter()
            .filter(|(_, log)| !log.deleted)
            .filter(|(session_id, log)| match cursor {
                Some(cursor) => {
                    log.created_at_ms < cursor.created_at_ms()
                        || (log.created_at_ms == cursor.created_at_ms()
                            && session_id.as_str() < cursor.session_id())
                }
                None => true,
            })
            .map(|(session_id, log)| SessionListItem {
                session_id: session_id.clone(),
                version: log.verified.stream_version,
                status: log.verified.work_status().to_owned(),
                created_at_ms: log.created_at_ms,
                selection: log.verified.selection.clone(),
                workspace: log.verified.workspace.clone(),
            })
            .collect::<Vec<_>>();
        items.sort_by(|left, right| {
            right
                .created_at_ms
                .cmp(&left.created_at_ms)
                .then_with(|| right.session_id.cmp(&left.session_id))
        });
        let has_more = items.len() > limit;
        items.truncate(limit);
        let next_cursor = if has_more {
            let last = items.last().ok_or(StoreError::InvalidSessionListCursor)?;
            Some(SessionListCursor::new(
                last.created_at_ms,
                last.session_id.clone(),
            )?)
        } else {
            None
        };
        Ok(SessionListPage { items, next_cursor })
    }

    fn append_handoff_verified(
        &self,
        stream_id: &str,
        current: VerifiedSessionState,
        events: &[EventDraft],
    ) -> Result<SessionAppendResult, StoreError> {
        require_text("stream_id", stream_id)?;
        validate_event_drafts(events)?;
        if !matches!(
            events.first().map(|draft| &draft.event),
            Some(SessionEvent::ContextHandoffCreated { .. })
        ) || !matches!(
            events.last().map(|draft| &draft.event),
            Some(SessionEvent::ModelRequestCompleted { .. })
        ) || events
            .iter()
            .filter(|draft| matches!(draft.event, SessionEvent::ContextHandoffCreated { .. }))
            .count()
            != 1
        {
            return Err(StoreError::InvalidSessionStream);
        }
        let expected_version = current.stream_version;
        let session_lock = self.session_lock(stream_id)?;
        let _session_guard = session_lock.lock().map_err(|_| StoreError::Poisoned)?;
        let mut inner = self.lock()?;
        let log = require_session_mut(&mut inner, stream_id)?;
        if let Some(replayed) = replay_if_stored(stream_id, log, events)? {
            return Ok(replayed);
        }
        if expected_version != log.verified.stream_version {
            return Err(StoreError::OptimisticConcurrency {
                stream_id: stream_id.to_owned(),
                expected: expected_version,
                actual: log.verified.stream_version,
            });
        }
        let preverified = carried_state(stream_id, current, log)?;
        let first_id =
            parse_canonical_ulid(&events.first().ok_or(StoreError::EmptyEventBatch)?.event_id)?;
        if first_id <= log.last_event_id {
            return Err(StoreError::InvalidSessionStream);
        }
        let batch_size = u32::try_from(events.len().checked_add(1).ok_or(
            StoreError::IntegerRange {
                field: "handoff batch length",
            },
        )?)
        .map_err(|_| StoreError::IntegerRange {
            field: "handoff batch length",
        })?;
        let (rehydrated, records, mut persisted) =
            apply_event_batch(stream_id, expected_version, preverified, events, batch_size)?;
        let verified = verified_state(rehydrated)?;
        let last_domain = records.last().ok_or(StoreError::EmptyEventBatch)?;
        let event_id = crate::ids::new_ulid();
        let event_ulid = parse_canonical_ulid(&event_id)?;
        if event_ulid <= parse_canonical_ulid(&last_domain.event_id)? {
            return Err(StoreError::InvalidSessionStream);
        }
        let snapshot = JsonlEvent::Snapshot {
            event_id: event_id.clone(),
            stream_id: stream_id.to_owned(),
            batch_index: u32::try_from(events.len()).map_err(|_| StoreError::IntegerRange {
                field: "handoff snapshot batch index",
            })?,
            batch_size,
            through_event_id: last_domain.event_id.clone(),
            through_stream_version: verified.stream_version,
            state_schema_version: STATE_SCHEMA_VERSION,
            reducer_schema_version: REDUCER_SCHEMA_VERSION,
            state: Box::new((*verified).clone()),
        };
        persisted.push(snapshot);
        let bytes = serialize_jsonl(&persisted)?;
        let byte_len = u64::try_from(bytes.len()).map_err(|_| StoreError::Backend)?;
        let mut sealed = None;
        if log.current_bytes > self.segment_target_bytes {
            let old_current = log
                .segments
                .last()
                .ok_or(StoreError::InvalidSessionStream)?
                .current_path()?
                .to_path_buf();
            let new_path = create_segment(
                &self.root,
                stream_id,
                &records.first().ok_or(StoreError::EmptyEventBatch)?.event_id,
                &bytes,
            )?;
            log.segments.push(SegmentDescriptor {
                id: first_id,
                plain: Some(new_path),
                compressed: None,
            });
            log.current_bytes = byte_len;
            sealed = Some(old_current);
        } else {
            let current = log
                .segments
                .last()
                .ok_or(StoreError::InvalidSessionStream)?
                .current_path()?;
            append_bytes(current, &bytes)?;
            log.current_bytes = log
                .current_bytes
                .checked_add(byte_len)
                .ok_or(StoreError::Backend)?;
        }
        log.last_event_id = event_ulid;
        log.last_domain_event_id = parse_canonical_ulid(&last_domain.event_id)?;
        log.verified = verified.clone();
        let stream_version = verified.stream_version;
        drop(inner);
        if let Some(path) = sealed {
            self.compressor.enqueue(path);
        }
        Ok(SessionAppendResult {
            append: AppendResult {
                stream_id: stream_id.to_owned(),
                events: records,
                stream_version,
                replayed: false,
            },
            state: verified,
        })
    }
}

fn empty_rehydrated(session_id: &str) -> RehydratedState {
    RehydratedState {
        state: SessionState::new(session_id),
        prefix_digest: prefix_digest_seed(session_id),
        digest_components: StateDigestComponents::empty(),
    }
}

fn verified_state(rehydrated: RehydratedState) -> Result<VerifiedSessionState, StoreError> {
    rehydrated.state.validate()?;
    let state_digest = state_digest_v7(&rehydrated.state, &rehydrated.digest_components)?;
    Ok(VerifiedSessionState {
        state: Arc::new(rehydrated.state),
        prefix_digest: rehydrated.prefix_digest,
        state_digest_version: STATE_DIGEST_VERSION,
        state_digest,
        digest_components: rehydrated.digest_components,
    })
}

fn apply_domain_record(
    rehydrated: &mut RehydratedState,
    record: &EventRecord,
) -> Result<(), StoreError> {
    let expected_version = rehydrated
        .state
        .stream_version
        .checked_add(1)
        .ok_or(StoreError::InvalidSessionStream)?;
    if record.event_schema_version != EVENT_SCHEMA_VERSION
        || record.stream_id != rehydrated.state.session_id
        || record.stream_version != expected_version
    {
        return Err(StoreError::InvalidSessionStream);
    }
    record.event.validate()?;
    validate_created_object_identity(&record.event_id, &record.event)?;
    let previous_transcript_len = rehydrated.state.transcript.len();
    let payload = serde_json::to_vec(&record.event)?;
    let fingerprint = event_fingerprint(
        &record.stream_id,
        record.stream_version,
        &record.event_id,
        record.event_schema_version,
        record.event.kind(),
        &payload,
    );
    let previous_state =
        std::mem::replace(&mut rehydrated.state, SessionState::new(&record.stream_id));
    rehydrated.state = previous_state.apply_record_from_valid_state_owned(record)?;
    rehydrated.digest_components.update_after_event(
        &rehydrated.state,
        previous_transcript_len,
        &record.event,
    )?;
    rehydrated.prefix_digest = extend_prefix_digest(&rehydrated.prefix_digest, &fingerprint);
    Ok(())
}

fn parse_canonical_ulid(value: &str) -> Result<Ulid, StoreError> {
    let id = Ulid::from_string(value).map_err(|_| StoreError::Backend)?;
    if id.to_string() != value {
        return Err(StoreError::Backend);
    }
    Ok(id)
}

fn parse_lookup_ulid(value: &str) -> Result<Ulid, StoreError> {
    let id = Ulid::from_string(value).map_err(|_| StoreError::InvalidEventId)?;
    if id.to_string() != value {
        return Err(StoreError::InvalidEventId);
    }
    Ok(id)
}

fn observe_canonical_ulid(value: &str) -> Result<(), StoreError> {
    crate::ids::observe_ulid(parse_canonical_ulid(value)?);
    Ok(())
}

fn event_fingerprint(
    stream_id: &str,
    stream_version: StreamVersion,
    event_id: &str,
    event_schema_version: u32,
    event_type: &str,
    payload: &[u8],
) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(b"zork:event-fingerprint:v3");
    hash_field(&mut hasher, stream_id.as_bytes());
    hasher.update(stream_version.to_be_bytes());
    hash_field(&mut hasher, event_id.as_bytes());
    hasher.update(event_schema_version.to_be_bytes());
    hash_field(&mut hasher, event_type.as_bytes());
    hash_field(&mut hasher, payload);
    hasher.finalize().to_vec()
}

fn prefix_digest_seed(stream_id: &str) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(b"zork:event-prefix:v1");
    hash_field(&mut hasher, stream_id.as_bytes());
    hasher.finalize().to_vec()
}

fn snapshot_prefix_seed(
    stream_id: &str,
    snapshot_event_id: &str,
    stream_version: StreamVersion,
) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(b"zork:event-prefix-from-snapshot:v1");
    hash_field(&mut hasher, stream_id.as_bytes());
    hash_field(&mut hasher, snapshot_event_id.as_bytes());
    hasher.update(stream_version.to_be_bytes());
    hasher.finalize().to_vec()
}

fn extend_prefix_digest(previous: &[u8], event_fingerprint: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(b"zork:event-prefix-link:v1");
    hash_field(&mut hasher, previous);
    hash_field(&mut hasher, event_fingerprint);
    hasher.finalize().to_vec()
}

impl StateDigestComponents {
    fn empty() -> Self {
        Self {
            transcript: transcript_digest_seed(),
        }
    }

    fn from_state(state: &SessionState) -> Result<Self, StoreError> {
        let transcript = state
            .transcript
            .iter()
            .try_fold(transcript_digest_seed(), extend_transcript_digest)?;
        Ok(Self { transcript })
    }

    fn update_after_event(
        &mut self,
        state: &SessionState,
        previous_transcript_len: usize,
        event: &SessionEvent,
    ) -> Result<(), StoreError> {
        if matches!(event, SessionEvent::ContextHandoffCreated { .. }) {
            *self = Self::from_state(state)?;
            return Ok(());
        }
        for message in state
            .transcript
            .get(previous_transcript_len..)
            .ok_or_else(|| StoreError::RehydrationIntegrity {
                stream_id: state.session_id.clone(),
                version: state.stream_version,
            })?
        {
            self.transcript = extend_transcript_digest(self.transcript, message)?;
        }
        let _ = event;
        Ok(())
    }
}

fn transcript_digest_seed() -> IntegrityDigest {
    Sha256::digest(b"zork:session-transcript-digest:v1").into()
}

fn extend_transcript_digest<T: Serialize>(
    previous: IntegrityDigest,
    message: &T,
) -> Result<IntegrityDigest, StoreError> {
    let mut hasher = Sha256::new();
    hasher.update(b"zork:session-transcript-link:v1");
    hash_field(&mut hasher, &previous);
    hash_field(&mut hasher, &serde_json::to_vec(message)?);
    Ok(hasher.finalize().into())
}

fn state_digest_v7(
    state: &SessionState,
    components: &StateDigestComponents,
) -> Result<Vec<u8>, StoreError> {
    let projection = (
        (STATE_SCHEMA_VERSION, REDUCER_SCHEMA_VERSION),
        (
            &state.session_id,
            &state.created_at_ms,
            &state.selection,
            &state.system_prompt,
            &state.workspace,
        ),
        (
            &components.transcript,
            &state.last_model_attempt_failure,
            &state.mailbox,
            state.consumed_through_mailbox_seq,
            &state.active_wait,
            &state.active_timer,
            &state.wake_pending_wait_id,
            &state.inflight_tool_call_ids,
        ),
        (
            &state.active_activation,
            &state.last_activation_outcome,
            &state.active_model_round,
            &state.pending_context_handoff,
            &state.latest_context_handoff,
            &state.latest_model_usage,
            &state.last_context_handoff_failure,
            &state.last_model_attempts_exhausted,
        ),
        state.stream_version,
    );
    let mut hasher = Sha256::new();
    hasher.update(b"zork:session-state-digest:v7");
    hash_field(&mut hasher, &serde_json::to_vec(&projection)?);
    Ok(hasher.finalize().to_vec())
}

fn state_digest_for_version(
    state: &SessionState,
    components: &StateDigestComponents,
    version: i64,
) -> Result<Vec<u8>, StoreError> {
    if version != STATE_DIGEST_VERSION {
        return Err(StoreError::InvalidIntegrityAnchor {
            stream_id: state.session_id.clone(),
            version: state.stream_version,
        });
    }
    state_digest_v7(state, components)
}

fn digest_matches(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .fold(0_u8, |difference, (left, right)| {
                difference | (*left ^ *right)
            })
            == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::state::{
        ContextHandoffDocument, ContextHandoffPlan, MailboxMessage, ModelRequestPurpose,
        SessionSelection,
    };
    use std::time::{Duration, Instant};

    fn selection() -> SessionSelection {
        SessionSelection {
            profile_id: "profile".into(),
            model: "model".into(),
            thinking: "off".into(),
        }
    }

    fn mailbox_message(message_id: impl Into<String>, content: impl Into<String>) -> SessionEvent {
        SessionEvent::MailboxMessageAppended {
            message: MailboxMessage {
                message_id: message_id.into(),
                mailbox_seq: 1,
                content: Arc::from(content.into()),
                received_at_ms: 2,
            },
        }
    }

    fn mailbox_draft(content: impl Into<String>) -> EventDraft {
        let content = content.into();
        EventDraft::identified(|event_id| mailbox_message(event_id, content))
    }

    fn mailbox_drain_batch(content: impl Into<String>) -> Vec<EventDraft> {
        let content = content.into();
        EventDraft::batch(2, |event_ids| {
            vec![
                mailbox_message(&event_ids[0], content),
                SessionEvent::MailboxDrained {
                    through_mailbox_seq: 1,
                },
            ]
        })
    }

    fn prepare_handoff(
        store: &JsonlEventStore,
        session_id: &str,
        mut state: VerifiedSessionState,
    ) -> (VerifiedSessionState, Vec<EventDraft>) {
        if state.mailbox.is_empty() {
            let seq = state.consumed_through_mailbox_seq + 1;
            let input = EventDraft::batch(2, |event_ids| {
                vec![
                    SessionEvent::MailboxMessageAppended {
                        message: MailboxMessage {
                            message_id: event_ids[0].clone(),
                            mailbox_seq: seq,
                            content: Arc::from("handoff boundary"),
                            received_at_ms: 2,
                        },
                    },
                    SessionEvent::MailboxDrained {
                        through_mailbox_seq: seq,
                    },
                ]
            });
            state = store
                .append_verified(session_id, state, &input)
                .unwrap()
                .state;
        } else {
            let through = state.mailbox.last().unwrap().mailbox_seq;
            state = store
                .append_verified(
                    session_id,
                    state,
                    &EventDraft::single(SessionEvent::MailboxDrained {
                        through_mailbox_seq: through,
                    }),
                )
                .unwrap()
                .state;
        }
        let boundary = state.transcript.last().unwrap().message_id.clone();
        let version = state.stream_version;
        let previous_handoff_id = state
            .latest_context_handoff
            .as_ref()
            .map(|handoff| handoff.handoff_id.clone());
        let next_generation = state
            .latest_context_handoff
            .as_ref()
            .map_or(2, |handoff| handoff.next_generation + 1);
        let selection = state.selection.clone();
        let mailbox_through_seq = state.consumed_through_mailbox_seq;
        let preparation = EventDraft::batch(5, |event_ids| {
            let activation_id = event_ids[0].clone();
            let plan_id = event_ids[1].clone();
            let round_id = event_ids[2].clone();
            let request_id = event_ids[3].clone();
            vec![
                SessionEvent::ActivationStarted {
                    activation_id: activation_id.clone(),
                    selection: selection.clone(),
                    started_at_ms: 3,
                },
                SessionEvent::ContextHandoffPlanned {
                    plan: ContextHandoffPlan {
                        plan_id: plan_id.clone(),
                        activation_id: activation_id.clone(),
                        previous_handoff_id: previous_handoff_id.clone(),
                        next_generation,
                        covered_through_message_id: boundary.clone(),
                        max_output_tokens: 128_000,
                        selection: selection.clone(),
                    },
                },
                SessionEvent::ModelRoundStarted {
                    activation_id: activation_id.clone(),
                    round_id: round_id.clone(),
                    purpose: ModelRequestPurpose::ContextHandoff,
                    mailbox_through_seq,
                    started_at_ms: 4,
                },
                SessionEvent::ModelRequestDeclared {
                    activation_id: activation_id.clone(),
                    round_id: round_id.clone(),
                    request_id: request_id.clone(),
                    request_fingerprint: format!("request-fingerprint-{version}"),
                    prompt_fingerprint: format!("prompt-fingerprint-{version}"),
                    tool_schema_fingerprint: format!("tool-fingerprint-{version}"),
                    maximum_attempts: 1,
                },
                SessionEvent::ModelAttemptStarted {
                    activation_id,
                    round_id,
                    request_id,
                    attempt_id: event_ids[4].clone(),
                    attempt_number: 1,
                    started_at_ms: 5,
                },
            ]
        });
        let activation_id = preparation[0].event_id.clone();
        let plan_id = preparation[1].event_id.clone();
        let round_id = preparation[2].event_id.clone();
        let request_id = preparation[3].event_id.clone();
        let attempt_id = preparation[4].event_id.clone();
        state = store
            .append_verified(session_id, state, &preparation)
            .unwrap()
            .state;
        let final_events = EventDraft::batch(2, |event_ids| {
            vec![
                SessionEvent::ContextHandoffCreated {
                    handoff: ContextHandoffDocument {
                        handoff_id: event_ids[0].clone(),
                        plan_id: plan_id.clone(),
                        previous_handoff_id: previous_handoff_id.clone(),
                        next_generation,
                        covered_through_message_id: boundary.clone(),
                        document: "compact handoff document".to_owned(),
                        document_tokens: Some(8),
                        selection: selection.clone(),
                    },
                },
                SessionEvent::ModelRequestCompleted {
                    activation_id: activation_id.clone(),
                    round_id: round_id.clone(),
                    request_id: request_id.clone(),
                    attempt_id: attempt_id.clone(),
                    usage: None,
                    provider_input: None,
                },
            ]
        });
        (state, final_events)
    }

    fn wait_for_path(path: &Path) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !path.exists() {
            assert!(
                Instant::now() < deadline,
                "{} was not created",
                path.display()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn wait_for_absent(path: &Path) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while path.exists() {
            assert!(
                Instant::now() < deadline,
                "{} was not removed",
                path.display()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn segment_files(root: &Path, session_id: &str) -> Vec<PathBuf> {
        let mut paths = fs::read_dir(segments_dir(root, session_id))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                let name = path.file_name().unwrap().to_string_lossy();
                name.ends_with(".jsonl") || name.ends_with(".jsonl.zst")
            })
            .collect::<Vec<_>>();
        paths.sort();
        paths
    }

    fn read_plain_events(path: &Path) -> Vec<JsonlEvent> {
        fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    #[test]
    fn current_segment_metadata_scan_is_bounded_by_one_jsonl_line() {
        let root = tempfile::tempdir().unwrap();
        let session_id = crate::ids::new_ulid();
        let mut path = None;
        let mut largest_line = 0_u64;
        for index in 0..512_u64 {
            let event_id = crate::ids::new_ulid();
            let row = JsonlEvent::Domain {
                event_id: event_id.clone(),
                stream_id: session_id.clone(),
                stream_version: index + 1,
                event_schema_version: EVENT_SCHEMA_VERSION,
                batch_index: 0,
                batch_size: 1,
                event: Box::new(SessionEvent::MailboxMessageAppended {
                    message: MailboxMessage {
                        message_id: event_id.clone(),
                        mailbox_seq: index + 1,
                        content: Arc::from("x".repeat(32 * 1024)),
                        received_at_ms: 2,
                    },
                }),
            };
            let bytes = serialize_jsonl(&[row]).unwrap();
            largest_line = largest_line.max(bytes.len() as u64);
            if let Some(path) = path.as_deref() {
                append_bytes(path, &bytes).unwrap();
            } else {
                path = Some(create_segment(root.path(), &session_id, &event_id, &bytes).unwrap());
            }
        }
        let path = path.expect("current segment");

        let scan = scan_current_segment_metadata(&path).unwrap();

        assert_eq!(scan.complete_bytes, fs::metadata(&path).unwrap().len());
        assert!(scan.complete_bytes > SESSION_SEGMENT_TARGET_BYTES);
        assert_eq!(scan.max_line_bytes, largest_line);
        assert!(scan.max_line_bytes * 100 < scan.complete_bytes);
        assert!(scan.latest_snapshot.is_none());
    }

    #[test]
    fn stores_one_jsonl_event_per_line_without_a_commit_envelope() {
        let root = tempfile::tempdir().unwrap();
        let store = JsonlEventStore::open(root.path()).unwrap();
        let created = store
            .create_session(&SessionCreate {
                created_at_ms: 1,
                selection: selection(),
                system_prompt: None,
                workspace: "/workspace".to_owned(),
            })
            .unwrap();
        let session_id = created.state.session_id.clone();
        let segments = segment_files(root.path(), &session_id);
        assert_eq!(segments.len(), 1);
        let raw = fs::read_to_string(&segments[0]).unwrap();
        let value: serde_json::Value = serde_json::from_str(raw.trim_end()).unwrap();
        assert_eq!(value["kind"], "domain");
        assert!(value.get("commit_id").is_none());
        assert!(value.get("events").is_none());
        let event_id = value["event_id"].as_str().unwrap();
        assert_eq!(event_id, session_id);
        assert_eq!(value["event"]["session_id"], session_id);
        assert_eq!(
            segments[0].file_name().unwrap().to_string_lossy(),
            format!("{event_id}.jsonl")
        );
    }

    #[test]
    fn rejects_a_second_identity_for_an_event_created_object() {
        let draft = EventDraft {
            event_id: crate::ids::new_ulid(),
            event: SessionEvent::MailboxMessageAppended {
                message: MailboxMessage {
                    message_id: crate::ids::new_ulid(),
                    mailbox_seq: 1,
                    content: Arc::from("message"),
                    received_at_ms: 1,
                },
            },
        };

        assert!(matches!(
            validate_event_drafts(std::slice::from_ref(&draft)),
            Err(StoreError::InvalidSessionStream)
        ));
    }

    #[test]
    fn replay_rejects_a_second_identity_for_an_event_created_object() {
        let session_id = crate::ids::new_ulid();
        let event_id = crate::ids::new_ulid();
        let mut rehydrated = empty_rehydrated(&session_id);
        let record = EventRecord {
            stream_id: session_id.clone(),
            stream_version: 1,
            event_id,
            event_schema_version: EVENT_SCHEMA_VERSION,
            batch_index: 0,
            batch_size: 1,
            event: SessionEvent::SessionCreated {
                schema_version: SESSION_CREATED_SCHEMA_VERSION,
                session_id,
                created_at_ms: 1,
                selection: selection(),
                system_prompt: None,
                workspace: "/workspace".to_owned(),
            },
        };

        assert!(matches!(
            apply_domain_record(&mut rehydrated, &record),
            Err(StoreError::InvalidSessionStream)
        ));
    }

    #[test]
    fn multi_event_append_is_multiple_lines_and_exact_retry_is_idempotent() {
        let root = tempfile::tempdir().unwrap();
        let store = JsonlEventStore::open(root.path()).unwrap();
        let created = store
            .create_session(&SessionCreate {
                created_at_ms: 1,
                selection: selection(),
                system_prompt: None,
                workspace: "/workspace".to_owned(),
            })
            .unwrap();
        let session_id = created.state.session_id.clone();
        let events = mailbox_drain_batch("one batch");
        let stale = store.rehydrate_verified(&session_id).unwrap();
        let appended = store
            .append_verified(&session_id, stale.clone(), &events)
            .unwrap();
        let path = segment_files(root.path(), &session_id).pop().unwrap();
        let raw = fs::read_to_string(&path).unwrap();
        let rows = raw
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1]["batch_index"], 0);
        assert_eq!(rows[1]["batch_size"], 2);
        assert_eq!(rows[2]["batch_index"], 1);
        assert_eq!(rows[2]["batch_size"], 2);
        assert!(rows.iter().all(|row| row.get("commit_id").is_none()));

        let replayed = store.append_verified(&session_id, stale, &events).unwrap();
        assert!(replayed.append.replayed);
        assert_eq!(replayed.append.events, appended.append.events);
        assert_eq!(fs::read_to_string(path).unwrap(), raw);

        let mut conflicting = events.clone();
        conflicting[0].event = mailbox_message(conflicting[0].event_id.clone(), "different body");
        assert!(matches!(
            store.append(&session_id, &created.state, &conflicting),
            Err(StoreError::EventBatchConflict { event_id })
                if event_id == events[0].event_id
        ));
    }

    #[test]
    fn current_recovery_removes_every_line_of_an_incomplete_final_batch() {
        let root = tempfile::tempdir().unwrap();
        let store = JsonlEventStore::open(root.path()).unwrap();
        let created = store
            .create_session(&SessionCreate {
                created_at_ms: 1,
                selection: selection(),
                system_prompt: None,
                workspace: "/workspace".to_owned(),
            })
            .unwrap();
        let session_id = created.state.session_id.clone();
        let events = EventDraft::batch(3, |event_ids| {
            vec![
                mailbox_message(&event_ids[0], "incomplete"),
                SessionEvent::MailboxDrained {
                    through_mailbox_seq: 1,
                },
                SessionEvent::MailboxDrained {
                    through_mailbox_seq: 1,
                },
            ]
        });
        store.append(&session_id, &created.state, &events).unwrap();
        let path = segment_files(root.path(), &session_id).pop().unwrap();
        let lines = fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        fs::write(&path, format!("{}\n{}\n", lines[0], lines[1])).unwrap();
        drop(store);

        let reopened = JsonlEventStore::open(root.path()).unwrap();
        let recovered = reopened.rehydrate(&session_id).unwrap();
        assert_eq!(recovered.stream_version, 1);
        assert!(recovered.mailbox.is_empty());
        assert_eq!(fs::read_to_string(path).unwrap(), format!("{}\n", lines[0]));
    }

    #[test]
    fn current_recovery_removes_the_whole_incomplete_handoff_batch() {
        let root = tempfile::tempdir().unwrap();
        let store = JsonlEventStore::open_with_segment_target(root.path(), u64::MAX).unwrap();
        let created = store
            .create_session(&SessionCreate {
                created_at_ms: 1,
                selection: selection(),
                system_prompt: None,
                workspace: "/workspace".to_owned(),
            })
            .unwrap();
        let session_id = created.state.session_id.clone();
        let state = store.rehydrate_verified(&session_id).unwrap();
        let (before_handoff, handoff) = prepare_handoff(&store, &session_id, state);
        let before_version = before_handoff.stream_version;
        store
            .append_handoff_verified(&session_id, before_handoff, &handoff)
            .unwrap();

        let path = segment_files(root.path(), &session_id).pop().unwrap();
        let lines = fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let final_rows = lines[lines.len() - 3..]
            .iter()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(final_rows[0]["batch_index"], 0);
        assert_eq!(final_rows[1]["batch_index"], 1);
        assert_eq!(final_rows[2]["batch_index"], 2);
        assert!(final_rows.iter().all(|row| row["batch_size"] == 3));

        // Simulate a crash after both domain rows reached the current segment
        // but before the snapshot row from the same physical write was durable.
        fs::write(&path, format!("{}\n", lines[..lines.len() - 1].join("\n"))).unwrap();
        drop(store);

        let reopened = JsonlEventStore::open(root.path()).unwrap();
        let recovered = reopened.rehydrate_verified(&session_id).unwrap();
        assert_eq!(recovered.stream_version, before_version);
        assert!(recovered.latest_context_handoff.is_none());
        assert!(recovered.pending_context_handoff.is_some());
        assert_eq!(
            fs::read_to_string(&path).unwrap().lines().count(),
            lines.len() - 3
        );

        let retried = reopened
            .append_handoff_verified(&session_id, recovered, &handoff)
            .unwrap();
        assert!(retried.state.latest_context_handoff.is_some());
    }

    #[test]
    fn ordinary_events_do_not_roll_an_oversized_segment() {
        let root = tempfile::tempdir().unwrap();
        let store = JsonlEventStore::open_with_segment_target(root.path(), 2 * 1024).unwrap();
        let created = store
            .create_session(&SessionCreate {
                created_at_ms: 1,
                selection: selection(),
                system_prompt: None,
                workspace: "/workspace".to_owned(),
            })
            .unwrap();
        let session_id = created.state.session_id.clone();
        store
            .append(
                &session_id,
                &created.state,
                &[mailbox_draft("x".repeat(4 * 1024))],
            )
            .unwrap();
        let segments = segment_files(root.path(), &session_id);
        assert_eq!(segments.len(), 1);
        assert!(fs::metadata(&segments[0]).unwrap().len() > 2 * 1024);
    }

    #[test]
    fn snapshot_stays_in_the_current_segment_at_or_below_the_target() {
        let root = tempfile::tempdir().unwrap();
        let store = JsonlEventStore::open_with_segment_target(root.path(), u64::MAX).unwrap();
        let created = store
            .create_session(&SessionCreate {
                created_at_ms: 1,
                selection: selection(),
                system_prompt: None,
                workspace: "/workspace".to_owned(),
            })
            .unwrap();
        let session_id = created.state.session_id.clone();
        let state = store.rehydrate_verified(&session_id).unwrap();
        let (state, handoff) = prepare_handoff(&store, &session_id, state);
        let appended = store
            .append_handoff_verified(&session_id, state, &handoff)
            .unwrap();
        let replayed = SessionState::replay(
            session_id.clone(),
            store.read_stream(&session_id, 0, usize::MAX).unwrap(),
        )
        .unwrap();
        assert_eq!(replayed, appended.state.clone().into_state());
        let segments = segment_files(root.path(), &session_id);
        assert_eq!(segments.len(), 1);
        let events = read_plain_events(&segments[0]);
        assert!(matches!(events.last(), Some(JsonlEvent::Snapshot { .. })));
        let snapshot: serde_json::Value = serde_json::from_str(
            fs::read_to_string(&segments[0])
                .unwrap()
                .lines()
                .last()
                .unwrap(),
        )
        .unwrap();
        assert!(snapshot.get("checksum").is_none());
        assert!(snapshot.get("encoding").is_none());
        assert_eq!(snapshot["state"]["workspace"], "/workspace");
        assert!(!snapshot.to_string().contains("compact handoff document"));
        assert!(snapshot["state"]["transcript"]
            .as_array()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn snapshot_does_not_roll_when_current_size_equals_the_target() {
        let root = tempfile::tempdir().unwrap();
        let store = JsonlEventStore::open(root.path()).unwrap();
        let created = store
            .create_session(&SessionCreate {
                created_at_ms: 1,
                selection: selection(),
                system_prompt: None,
                workspace: "/workspace".to_owned(),
            })
            .unwrap();
        let session_id = created.state.session_id.clone();
        let state = store.rehydrate_verified(&session_id).unwrap();
        let (state, handoff) = prepare_handoff(&store, &session_id, state);
        let current = segment_files(root.path(), &session_id).remove(0);
        let exact_size = fs::metadata(current).unwrap().len();
        drop(store);

        let store = JsonlEventStore::open_with_segment_target(root.path(), exact_size).unwrap();
        store
            .append_handoff_verified(&session_id, state, &handoff)
            .unwrap();
        assert_eq!(segment_files(root.path(), &session_id).len(), 1);
    }

    #[test]
    fn snapshot_rolls_an_oversized_segment_and_starts_the_new_segment() {
        assert_eq!(SESSION_SEGMENT_TARGET_BYTES, 16 * 1024 * 1024);
        assert_eq!(HISTORICAL_SEGMENT_ZSTD_LEVEL, 12);
        let root = tempfile::tempdir().unwrap();
        let store = JsonlEventStore::open_with_segment_target(root.path(), 2 * 1024).unwrap();
        let created = store
            .create_session(&SessionCreate {
                created_at_ms: 1,
                selection: selection(),
                system_prompt: None,
                workspace: "/workspace".to_owned(),
            })
            .unwrap();
        let session_id = created.state.session_id.clone();
        let before_append = store.rehydrate_verified(&session_id).unwrap();
        let oversized = [mailbox_draft("x".repeat(4 * 1024))];
        let appended = store
            .append_verified(&session_id, before_append.clone(), &oversized)
            .unwrap();
        assert_eq!(segment_files(root.path(), &session_id).len(), 1);
        let (state, handoff) = prepare_handoff(&store, &session_id, appended.state);
        let first_handoff_event_id = handoff[0].event_id.clone();
        store
            .append_handoff_verified(&session_id, state, &handoff)
            .unwrap();
        assert!(!session_dir(root.path(), &session_id)
            .join("snapshot.json")
            .exists());

        let deadline = Instant::now() + Duration::from_secs(5);
        let segments = loop {
            let segments = segment_files(root.path(), &session_id);
            if segments.len() == 2
                && segments[0].to_string_lossy().ends_with(".jsonl.zst")
                && segments[1].to_string_lossy().ends_with(".jsonl")
            {
                break segments;
            }
            assert!(
                Instant::now() < deadline,
                "historical segment was not compressed"
            );
            std::thread::sleep(Duration::from_millis(10));
        };
        let next = read_plain_events(&segments[1]);
        let JsonlEvent::Domain {
            event_id, event, ..
        } = &next[0]
        else {
            panic!("new segment did not start with the handoff batch");
        };
        assert!(matches!(
            event.as_ref(),
            SessionEvent::ContextHandoffCreated { .. }
        ));
        assert!(matches!(next.last(), Some(JsonlEvent::Snapshot { .. })));
        assert_eq!(event_id, &first_handoff_event_id);
        assert_eq!(
            segments[1].file_name().unwrap().to_string_lossy(),
            format!("{event_id}.jsonl")
        );
    }

    #[test]
    fn event_ulid_reads_page_backwards_across_segments_and_locate_one_event() {
        let root = tempfile::tempdir().unwrap();
        let store = JsonlEventStore::open_with_segment_target(root.path(), 1).unwrap();
        let created = store
            .create_session(&SessionCreate {
                created_at_ms: 1,
                selection: selection(),
                system_prompt: None,
                workspace: "/workspace".to_owned(),
            })
            .unwrap();
        let session_id = created.state.session_id.clone();
        let state = store.rehydrate_verified(&session_id).unwrap();
        let (state, handoff) = prepare_handoff(&store, &session_id, state);
        let handoff_event_id = handoff[0].event_id.clone();
        let committed = store
            .append_handoff_verified(&session_id, state, &handoff)
            .unwrap();
        let suffix_seq = committed.state.consumed_through_mailbox_seq + 1;
        store
            .append_verified(
                &session_id,
                committed.state,
                &[EventDraft::identified(|message_id| {
                    SessionEvent::MailboxMessageAppended {
                        message: MailboxMessage {
                            message_id: message_id.to_owned(),
                            mailbox_seq: suffix_seq,
                            content: Arc::from("suffix"),
                            received_at_ms: 6,
                        },
                    }
                })],
            )
            .unwrap();

        let all = store.read_stream(&session_id, 0, usize::MAX).unwrap();
        let tail = store.read_stream_before(&session_id, None, 3).unwrap();
        assert_eq!(tail, all[all.len() - 3..]);

        let older = store
            .read_stream_before(&session_id, Some(&tail[0].event_id), 2)
            .unwrap();
        let boundary = all
            .iter()
            .position(|record| record.event_id == tail[0].event_id)
            .unwrap();
        assert_eq!(older, all[boundary.saturating_sub(2)..boundary]);

        assert_eq!(
            store.read_event(&session_id, &handoff_event_id).unwrap(),
            all.iter()
                .find(|record| record.event_id == handoff_event_id)
                .cloned()
        );
        assert_eq!(
            store
                .rehydrate(&session_id)
                .unwrap()
                .latest_context_handoff
                .unwrap()
                .handoff_id,
            handoff_event_id
        );
    }

    #[test]
    fn restart_uses_the_inline_snapshot_and_replays_its_suffix() {
        let root = tempfile::tempdir().unwrap();
        let store = JsonlEventStore::open_with_segment_target(root.path(), 2 * 1024).unwrap();
        let created = store
            .create_session(&SessionCreate {
                created_at_ms: 1,
                selection: selection(),
                system_prompt: None,
                workspace: "/workspace".to_owned(),
            })
            .unwrap();
        let session_id = created.state.session_id.clone();
        let appended = store
            .append(
                &session_id,
                &created.state,
                &[mailbox_draft("x".repeat(4 * 1024))],
            )
            .unwrap();
        let (state, handoff) = prepare_handoff(&store, &session_id, appended.state);
        let handoff = store
            .append_handoff_verified(&session_id, state, &handoff)
            .unwrap();
        let suffix = store
            .append(
                &session_id,
                &handoff.state,
                &[EventDraft::identified(|message_id| {
                    SessionEvent::MailboxMessageAppended {
                        message: MailboxMessage {
                            message_id: message_id.to_owned(),
                            mailbox_seq: handoff.state.consumed_through_mailbox_seq + 1,
                            content: Arc::from("suffix"),
                            received_at_ms: 6,
                        },
                    }
                })],
            )
            .unwrap();
        let expected = suffix.state.clone().into_state();
        drop(store);

        let reopened = JsonlEventStore::open(root.path()).unwrap();
        assert_eq!(reopened.rehydrate(&session_id).unwrap(), expected);
        assert_eq!(
            reopened
                .read_stream(&session_id, 0, usize::MAX)
                .unwrap()
                .len(),
            expected.stream_version as usize
        );
        assert_eq!(reopened.read_stream(&session_id, 0, 1).unwrap().len(), 1);
    }

    #[test]
    fn startup_resumes_plain_history_compression_and_prefers_zstd_duplicates() {
        let root = tempfile::tempdir().unwrap();
        let store = JsonlEventStore::open_with_segment_target(root.path(), 1).unwrap();
        let created = store
            .create_session(&SessionCreate {
                created_at_ms: 1,
                selection: selection(),
                system_prompt: None,
                workspace: "/workspace".to_owned(),
            })
            .unwrap();
        let session_id = created.state.session_id.clone();
        let state = store.rehydrate_verified(&session_id).unwrap();
        let (state, handoff) = prepare_handoff(&store, &session_id, state);
        let committed = store
            .append_handoff_verified(&session_id, state, &handoff)
            .unwrap();
        let expected_version = committed.state.stream_version;
        let historical_plain = segment_files(root.path(), &session_id).remove(0);
        let historical_zstd = compressed_segment_path(&historical_plain);
        wait_for_path(&historical_zstd);
        let mut decoder =
            zstd::stream::read::Decoder::new(File::open(&historical_zstd).unwrap()).unwrap();
        let mut decoded = Vec::new();
        decoder.read_to_end(&mut decoded).unwrap();
        drop(store);

        fs::write(&historical_plain, &decoded).unwrap();
        let reopened = JsonlEventStore::open(root.path()).unwrap();
        assert_eq!(
            reopened.rehydrate(&session_id).unwrap().stream_version,
            expected_version
        );
        wait_for_absent(&historical_plain);
        drop(reopened);

        fs::write(&historical_plain, decoded).unwrap();
        fs::remove_file(&historical_zstd).unwrap();
        let reopened = JsonlEventStore::open(root.path()).unwrap();
        assert_eq!(
            reopened.rehydrate(&session_id).unwrap().stream_version,
            expected_version
        );
        wait_for_path(&historical_zstd);
        wait_for_absent(&historical_plain);
    }

    #[test]
    fn sealed_segment_damage_is_reported_when_history_is_read() {
        let root = tempfile::tempdir().unwrap();
        let store = JsonlEventStore::open_with_segment_target(root.path(), 1).unwrap();
        let created = store
            .create_session(&SessionCreate {
                created_at_ms: 1,
                selection: selection(),
                system_prompt: None,
                workspace: "/workspace".to_owned(),
            })
            .unwrap();
        let session_id = created.state.session_id.clone();
        let state = store.rehydrate_verified(&session_id).unwrap();
        let (state, handoff) = prepare_handoff(&store, &session_id, state);
        store
            .append_handoff_verified(&session_id, state, &handoff)
            .unwrap();
        let historical_plain = segment_files(root.path(), &session_id).remove(0);
        let historical_zstd = compressed_segment_path(&historical_plain);
        wait_for_path(&historical_zstd);
        let mut decoder =
            zstd::stream::read::Decoder::new(File::open(&historical_zstd).unwrap()).unwrap();
        let mut torn = Vec::new();
        decoder.read_to_end(&mut torn).unwrap();
        assert_eq!(torn.pop(), Some(b'\n'));
        drop(store);
        fs::write(&historical_plain, torn).unwrap();
        fs::remove_file(&historical_zstd).unwrap();

        let reopened = JsonlEventStore::open(root.path()).unwrap();
        assert!(reopened.read_stream(&session_id, 0, usize::MAX).is_err());
    }

    #[test]
    fn repairs_a_partial_json_line_only_in_the_current_segment() {
        let root = tempfile::tempdir().unwrap();
        let store = JsonlEventStore::open(root.path()).unwrap();
        let created = store
            .create_session(&SessionCreate {
                created_at_ms: 1,
                selection: selection(),
                system_prompt: None,
                workspace: "/workspace".to_owned(),
            })
            .unwrap();
        let session_id = created.state.session_id.clone();
        let current = segment_files(root.path(), &session_id).pop().unwrap();
        let committed = fs::read(&current).unwrap();
        drop(store);
        let mut file = OpenOptions::new().append(true).open(&current).unwrap();
        file.write_all(b"{\"kind\":\"domain\"").unwrap();
        file.sync_all().unwrap();
        drop(file);

        let reopened = JsonlEventStore::open(root.path()).unwrap();
        assert_eq!(reopened.rehydrate(&session_id).unwrap().stream_version, 1);
        assert_eq!(fs::read(current).unwrap(), committed);
    }
}
