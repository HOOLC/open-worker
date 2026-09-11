use futures_util::{future::poll_fn, task::AtomicWaker};
use std::{
    collections::{HashMap, VecDeque},
    fmt,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex, Weak,
    },
    task::Poll,
};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn next_id() -> u64 {
    NEXT_ID
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
        .expect("subscription identity exhausted")
}

/// A revision is comparable only within one source incarnation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cursor {
    pub source: u64,
    pub revision: u64,
}

impl fmt::Display for Cursor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.source, self.revision)
    }
}

/// Business domains within a source. Object identity belongs to the source key.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Topics(u64);

impl Topics {
    pub const ALL: Self = Self(u64::MAX);
    pub const NONE: Self = Self(0);
    pub const fn new(bits: u64) -> Self {
        Self(bits)
    }
    pub const fn bits(self) -> u64 {
        self.0
    }
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
    fn indices(self) -> impl Iterator<Item = u32> {
        let mut bits = self.0;
        std::iter::from_fn(move || {
            if bits == 0 {
                return None;
            }
            let index = bits.trailing_zeros();
            bits &= bits - 1;
            Some(index)
        })
    }
}

impl std::ops::BitOr for Topics {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for Topics {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

pub struct Snapshot<S> {
    pub cursor: Cursor,
    pub value: Arc<S>,
}

impl<S> Clone for Snapshot<S> {
    fn clone(&self) -> Self {
        Self {
            cursor: self.cursor,
            value: self.value.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct JournalLimits {
    pub commits: usize,
    /// Includes each record's fixed storage and the publisher's payload estimate.
    pub bytes: usize,
}

impl Default for JournalLimits {
    fn default() -> Self {
        Self {
            commits: 512,
            bytes: 2 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResetReason {
    Initial,
    Lagged,
    Replaced,
}

pub struct Change<D> {
    pub revision: u64,
    pub topics: Topics,
    pub value: Arc<D>,
}

impl<D> Clone for Change<D> {
    fn clone(&self) -> Self {
        Self {
            revision: self.revision,
            topics: self.topics,
            value: self.value.clone(),
        }
    }
}

pub enum Changes<D> {
    Reset(ResetReason),
    /// Records are ordered; indices in a patch refer to the state after applying
    /// the preceding record. They may not be individually dropped/conflated.
    Delta {
        from: Cursor,
        records: Vec<Change<D>>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchId(u64);

impl BatchId {
    pub fn get(self) -> u64 {
        self.0
    }
}

pub struct Batch<S, D> {
    pub id: BatchId,
    pub snapshot: Snapshot<S>,
    pub topics: Topics,
    pub changes: Changes<D>,
}

impl<S, D> Batch<S, D> {
    pub fn is_reset(&self) -> bool {
        matches!(self.changes, Changes::Reset(_))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Closed;

impl fmt::Display for Closed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("state source closed")
    }
}

impl std::error::Error for Closed {}

struct Signal {
    waker: AtomicWaker,
    event: AtomicU64,
    closed: AtomicBool,
    detached: AtomicBool,
    urgent: AtomicBool,
}

impl Signal {
    fn advance(&self) {
        self.event.fetch_add(1, Ordering::Release);
    }
}

struct Listener {
    signal: Weak<Signal>,
    interests: Topics,
    latest: u64,
    pending: bool,
}

struct Record<D> {
    revision: u64,
    topics: Topics,
    value: Option<Arc<D>>,
    bytes: usize,
}

struct Owned<S, D> {
    snapshot: Snapshot<S>,
    records: VecDeque<Record<D>>,
    floor: u64,
    replaced_at: u64,
    bytes: usize,
    listeners: HashMap<u64, Listener>,
    all: Vec<u64>,
    topics: HashMap<u32, Vec<u64>>,
}

struct Shared<S, D> {
    owned: Mutex<Owned<S, D>>,
    limits: JournalLimits,
}

/// The sole publishing handle. Clone an Arc<Source> when multiple business
/// operations share it; subscriptions hold only its data, not this owner.
pub struct Source<S, D = ()> {
    shared: Arc<Shared<S, D>>,
}

impl<S, D> Source<S, D> {
    pub fn new(value: S, limits: JournalLimits) -> Self {
        Self {
            shared: Arc::new(Shared {
                limits,
                owned: Mutex::new(Owned {
                    snapshot: Snapshot {
                        cursor: Cursor {
                            source: next_id(),
                            revision: 0,
                        },
                        value: Arc::new(value),
                    },
                    records: VecDeque::new(),
                    floor: 0,
                    replaced_at: 0,
                    bytes: 0,
                    listeners: HashMap::new(),
                    all: Vec::new(),
                    topics: HashMap::new(),
                }),
            }),
        }
    }

    pub fn snapshot(&self) -> Snapshot<S> {
        self.shared
            .owned
            .lock()
            .expect("state source")
            .snapshot
            .clone()
    }

    pub fn observed(&self) -> bool {
        !self
            .shared
            .owned
            .lock()
            .expect("state source")
            .listeners
            .is_empty()
    }

    pub fn subscribe(&self) -> Subscription<S, D> {
        self.subscribe_topics(Topics::ALL).0
    }

    /// Registration and its opening snapshot share a critical section, so a
    /// delivery counter can start at registration without a read/subscribe race.
    pub fn subscribe_topics(&self, interests: Topics) -> (Subscription<S, D>, Snapshot<S>) {
        assert!(!interests.is_empty(), "subscription needs a business topic");
        let mut owned = self.shared.owned.lock().expect("state source");
        let opening = owned.snapshot.clone();
        let id = next_id();
        let signal = Arc::new(Signal {
            waker: AtomicWaker::new(),
            event: AtomicU64::new(1),
            closed: AtomicBool::new(false),
            detached: AtomicBool::new(false),
            urgent: AtomicBool::new(false),
        });
        owned.listeners.insert(
            id,
            Listener {
                signal: Arc::downgrade(&signal),
                interests,
                latest: opening.cursor.revision,
                pending: true,
            },
        );
        if interests == Topics::ALL {
            owned.all.push(id);
        } else {
            for topic in interests.indices() {
                owned.topics.entry(topic).or_default().push(id);
            }
        }
        let subscription = Subscription {
            shared: self.shared.clone(),
            id,
            interests,
            readiness: Readiness { signal, seen: 0 },
            applied: None,
            prepared: None,
        };
        (subscription, opening)
    }

    /// The reducer has already checked its touched fields for a real change.
    /// No deep equality, encoding or platform callback is performed here.
    pub fn publish(&self, value: S, changes: D, topics: Topics, payload_bytes: usize) {
        self.commit(value, Some(Arc::new(changes)), topics, payload_bytes, false);
    }

    /// A replacement has no valid patch from an earlier version.
    pub fn replace(&self, value: S) {
        self.commit(value, None, Topics::ALL, 0, false);
    }

    /// A control invalidation, such as revocation, must not wait for a visible frame.
    pub fn invalidate(&self, value: S) {
        self.commit(value, None, Topics::ALL, 0, true);
    }

    fn commit(
        &self,
        value: S,
        changes: Option<Arc<D>>,
        topics: Topics,
        payload_bytes: usize,
        urgent: bool,
    ) {
        assert!(
            !topics.is_empty(),
            "a commit must identify its changed topics"
        );
        let value = Arc::new(value);
        let mut owned = self.shared.owned.lock().expect("state source");
        let cursor = Cursor {
            source: owned.snapshot.cursor.source,
            revision: owned
                .snapshot
                .cursor
                .revision
                .checked_add(1)
                .expect("state revision exhausted"),
        };
        let replacing = changes.is_none();
        if replacing {
            owned.replaced_at = cursor.revision;
        }
        let previous = std::mem::replace(&mut owned.snapshot, Snapshot { cursor, value });
        let bytes = payload_bytes.saturating_add(std::mem::size_of::<Record<D>>());
        owned.bytes = owned.bytes.saturating_add(bytes);
        owned.records.push_back(Record {
            revision: cursor.revision,
            topics,
            value: changes,
            bytes,
        });
        let mut retired = Vec::new();
        while owned.records.len() > self.shared.limits.commits
            || owned.bytes > self.shared.limits.bytes
            || owned.listeners.is_empty()
        {
            let Some(record) = owned.records.pop_front() else {
                break;
            };
            owned.floor = record.revision;
            owned.bytes = owned.bytes.saturating_sub(record.bytes);
            retired.push(record);
        }
        let mut ids = owned.all.clone();
        for topic in topics.indices() {
            if let Some(listeners) = owned.topics.get(&topic) {
                ids.extend(listeners);
            }
        }
        ids.sort_unstable();
        ids.dedup();
        let mut wake = Vec::new();
        for id in ids {
            let listener = owned.listeners.get_mut(&id).expect("registered topic");
            listener.latest = cursor.revision;
            if !listener.pending || replacing {
                listener.pending = true;
                if let Some(signal) = listener.signal.upgrade() {
                    if urgent {
                        signal.urgent.store(true, Ordering::Release);
                    }
                    signal.advance();
                    wake.push(signal);
                }
            }
        }
        drop(owned);
        for signal in wake {
            signal.waker.wake();
        }
        // Destruction may release large immutable payloads; do it outside the lock.
        drop((previous, retired));
    }

    pub fn retained(&self) -> (usize, usize) {
        let owned = self.shared.owned.lock().expect("state source");
        (owned.records.len(), owned.bytes)
    }
}

impl<S, D> Drop for Source<S, D> {
    fn drop(&mut self) {
        let signals = {
            let owned = self.shared.owned.lock().expect("state source");
            owned
                .listeners
                .values()
                .filter_map(|l| l.signal.upgrade())
                .collect::<Vec<_>>()
        };
        for signal in signals {
            signal.closed.store(true, Ordering::Release);
            signal.waker.wake();
        }
    }
}

/// One waiter per subscription. Notification progress is independent of the
/// applied cursor; this can live in an adapter task while the UI owns the reader.
pub struct Readiness {
    signal: Arc<Signal>,
    seen: u64,
}

impl Readiness {
    pub fn take_urgent(&mut self) -> bool {
        self.signal.urgent.swap(false, Ordering::AcqRel)
    }

    pub async fn changed(&mut self) -> Result<(), Closed> {
        poll_fn(|cx| self.poll_changed(cx)).await
    }
    /// Aggregate a few independent readiness sources without allocating one
    /// future per source. Each source still permits just one active waiter.
    pub fn poll_changed(&mut self, cx: &mut std::task::Context<'_>) -> Poll<Result<(), Closed>> {
        self.signal.waker.register(cx.waker());
        if self.signal.detached.load(Ordering::Acquire) {
            return Poll::Ready(Err(Closed));
        }
        let event = self.signal.event.load(Ordering::Acquire);
        if self.seen != event {
            self.seen = event;
            Poll::Ready(Ok(()))
        } else if self.signal.closed.load(Ordering::Acquire) {
            Poll::Ready(Err(Closed))
        } else {
            Poll::Pending
        }
    }
}

pub struct Subscription<S, D = ()> {
    shared: Arc<Shared<S, D>>,
    id: u64,
    interests: Topics,
    readiness: Readiness,
    applied: Option<Cursor>,
    prepared: Option<Arc<Batch<S, D>>>,
}

impl<S, D> Subscription<S, D> {
    /// Use this instead of ready() when scheduling and reading live in separate
    /// places. There must still be only one active waiter for this subscription.
    pub fn readiness(&self) -> Readiness {
        let owned = self.shared.owned.lock().expect("state source");
        let event = self.readiness.signal.event.load(Ordering::Acquire);
        let pending = owned
            .listeners
            .get(&self.id)
            .expect("live subscription")
            .pending;
        Readiness {
            signal: self.readiness.signal.clone(),
            seen: if pending {
                event.saturating_sub(1)
            } else {
                event
            },
        }
    }

    pub async fn ready(&mut self) -> Result<(), Closed> {
        self.readiness.changed().await
    }
    pub fn applied(&self) -> Option<Cursor> {
        self.applied
    }

    /// Non-consuming read; prepare() pairs a snapshot with changes and a batch.
    pub fn snapshot(&self) -> Snapshot<S> {
        self.shared
            .owned
            .lock()
            .expect("state source")
            .snapshot
            .clone()
    }

    pub fn prepare(&mut self) -> Option<Arc<Batch<S, D>>> {
        let owned = self.shared.owned.lock().expect("state source");
        let retired = if self
            .prepared
            .as_ref()
            .is_some_and(|batch| batch.snapshot.cursor.revision < owned.replaced_at)
        {
            self.prepared.take()
        } else {
            None
        };
        if let Some(batch) = &self.prepared {
            return Some(batch.clone());
        }
        let listener = owned.listeners.get(&self.id).expect("live subscription");
        if self
            .applied
            .is_some_and(|cursor| cursor.revision >= listener.latest)
        {
            return None;
        }
        let mut topics = Topics::NONE;
        let changes = match self.applied {
            None => Changes::Reset(ResetReason::Initial),
            Some(from) if from.revision < owned.floor => Changes::Reset(ResetReason::Lagged),
            Some(from) if from.revision < owned.replaced_at => {
                Changes::Reset(ResetReason::Replaced)
            }
            Some(from) => {
                let mut records = Vec::new();
                let mut replaced = false;
                let skipped =
                    usize::try_from(from.revision - owned.floor).expect("bounded journal");
                for record in owned.records.range(skipped..) {
                    if !record.topics.intersects(self.interests) {
                        continue;
                    }
                    topics |= record.topics;
                    if let Some(value) = &record.value {
                        records.push(Change {
                            revision: record.revision,
                            topics: record.topics,
                            value: value.clone(),
                        });
                    } else {
                        replaced = true;
                        break;
                    }
                }
                if replaced {
                    Changes::Reset(ResetReason::Replaced)
                } else {
                    Changes::Delta { from, records }
                }
            }
        };
        if matches!(changes, Changes::Reset(_)) {
            topics = self.interests;
        } else {
            topics = Topics::new(topics.bits() & self.interests.bits());
        }
        let batch = Arc::new(Batch {
            id: BatchId(next_id()),
            snapshot: owned.snapshot.clone(),
            topics,
            changes,
        });
        drop(owned);
        drop(retired);
        self.prepared = Some(batch.clone());
        Some(batch)
    }

    pub fn acknowledge(&mut self, batch: BatchId) -> bool {
        self.finish(batch, true)
    }
    /// Ordinary newer commits keep a prepared result valid; replacement and
    /// revocation invalidate results already encoded on another thread.
    pub fn valid(&self, batch: BatchId) -> bool {
        self.prepared.as_ref().is_some_and(|prepared| {
            prepared.id == batch
                && prepared.snapshot.cursor.revision
                    >= self.shared.owned.lock().expect("state source").replaced_at
        })
    }
    pub fn discard(&mut self, batch: BatchId) -> bool {
        self.finish(batch, false)
    }

    fn finish(&mut self, batch: BatchId, applied: bool) -> bool {
        if self
            .prepared
            .as_ref()
            .is_none_or(|prepared| prepared.id != batch)
        {
            return false;
        }
        let prepared = self.prepared.take().unwrap();
        let mut owned = self.shared.owned.lock().expect("state source");
        let valid = prepared.snapshot.cursor.revision >= owned.replaced_at;
        if applied && valid {
            self.applied = Some(prepared.snapshot.cursor);
        }
        let listener = owned
            .listeners
            .get_mut(&self.id)
            .expect("live subscription");
        listener.pending = false;
        let again = self
            .applied
            .is_none_or(|cursor| cursor.revision < listener.latest);
        if again {
            listener.pending = true;
            self.readiness.signal.advance();
        }
        drop(owned);
        if again {
            self.readiness.signal.waker.wake();
        }
        valid
    }

    /// The platform lost its presentation mirror. The next read is a reset;
    /// outstanding results become stale and cannot advance this new baseline.
    pub fn reset(&mut self) {
        self.prepared = None;
        self.applied = None;
        let mut owned = self.shared.owned.lock().expect("state source");
        owned
            .listeners
            .get_mut(&self.id)
            .expect("live subscription")
            .pending = true;
        self.readiness.signal.advance();
        drop(owned);
        self.readiness.signal.waker.wake();
    }
}

impl<S, D> Drop for Subscription<S, D> {
    fn drop(&mut self) {
        let mut owned = self.shared.owned.lock().expect("state source");
        if let Some(listener) = owned.listeners.remove(&self.id) {
            if listener.interests == Topics::ALL {
                owned.all.retain(|id| *id != self.id);
            } else {
                for topic in listener.interests.indices() {
                    if let Some(ids) = owned.topics.get_mut(&topic) {
                        ids.retain(|id| *id != self.id);
                    }
                }
                owned.topics.retain(|_, ids| !ids.is_empty());
            }
        }
        let retired = if owned.listeners.is_empty() {
            owned.floor = owned.snapshot.cursor.revision;
            owned.bytes = 0;
            std::mem::take(&mut owned.records)
        } else {
            VecDeque::new()
        };
        drop(owned);
        drop(retired);
        self.readiness
            .signal
            .detached
            .store(true, Ordering::Release);
        self.readiness.signal.waker.wake();
    }
}
