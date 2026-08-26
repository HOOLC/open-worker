use super::*;

/// Receives provider text deltas for transient browser observation only. The
/// observer is never consulted by the durable reducer and its output is not
/// persisted, replayed, or included in session events.
pub trait ModelStreamObserver: Send + Sync + std::fmt::Debug {
    fn text_delta(&self, session_id: &str, activation_id: &str, round_id: &str, text: &str);
}

#[derive(Clone, Debug)]
pub(super) struct SilentModelStreamObserver;

impl ModelStreamObserver for SilentModelStreamObserver {
    fn text_delta(&self, _: &str, _: &str, _: &str, _: &str) {}
}

/// A bounded, best-effort model text update for currently attached clients.
/// It deliberately has no public cursor: reconnects replay durable facts only.
#[derive(Clone, Debug)]
pub struct TransientModelEvent {
    pub session_id: String,
    pub activation_id: String,
    pub round_id: String,
    pub text: String,
}

/// One ordered live-publication lane for durable event notifications and
/// best-effort model text. Durable facts remain authoritative in storage;
/// sharing the lane only preserves their causal boundary with transient text.
#[derive(Clone, Debug)]
pub enum RuntimeStreamEvent {
    Durable(Box<EventRecord>),
    Transient(TransientModelEvent),
}

#[derive(Clone, Debug)]
pub struct RuntimeStreamMessage {
    pub sequence: u64,
    pub event: RuntimeStreamEvent,
}

#[derive(Clone, Debug)]
pub struct RuntimeStreamFence {
    pub sequence: u64,
}

pub struct RuntimeStreamSubscription {
    pub receiver: broadcast::Receiver<RuntimeStreamMessage>,
    pub fence: RuntimeStreamFence,
}

#[derive(Debug, Default)]
struct RuntimeStreamPublisherState {
    sequence: u64,
}

/// Orders every live runtime publication and creates a replay/live fence while
/// publication is paused. Durable storage remains authoritative; the internal
/// sequence exists only to discard messages already covered by one replay
/// fence without reordering later transient progress.
#[derive(Debug)]
pub struct RuntimeStreamPublisher {
    sender: broadcast::Sender<RuntimeStreamMessage>,
    state: Mutex<RuntimeStreamPublisherState>,
}

impl RuntimeStreamPublisher {
    pub(super) fn new(capacity: usize) -> Arc<Self> {
        let (sender, _) = broadcast::channel(capacity);
        Arc::new(Self {
            sender,
            state: Mutex::new(RuntimeStreamPublisherState::default()),
        })
    }

    pub fn subscribe(&self) -> RuntimeStreamSubscription {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let receiver = self.sender.subscribe();
        RuntimeStreamSubscription {
            receiver,
            fence: RuntimeStreamFence {
                sequence: state.sequence,
            },
        }
    }

    pub(super) fn publish(&self, event: RuntimeStreamEvent) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.sequence = state
            .sequence
            .checked_add(1)
            .expect("runtime stream publication sequence exhausted");
        let _ = self.sender.send(RuntimeStreamMessage {
            sequence: state.sequence,
            event,
        });
    }
}

const MAX_TRANSIENT_TEXT_BYTES: usize = 16 * 1024;

#[derive(Clone, Debug)]
pub(super) struct BroadcastModelStreamObserver {
    pub(super) publisher: Arc<RuntimeStreamPublisher>,
}

impl ModelStreamObserver for BroadcastModelStreamObserver {
    fn text_delta(&self, session_id: &str, activation_id: &str, round_id: &str, text: &str) {
        if text.is_empty() {
            return;
        }
        let mut remaining = text;
        while !remaining.is_empty() {
            let mut end = remaining.len().min(MAX_TRANSIENT_TEXT_BYTES);
            while !remaining.is_char_boundary(end) {
                end -= 1;
            }
            let (chunk, rest) = remaining.split_at(end);
            self.publisher
                .publish(RuntimeStreamEvent::Transient(TransientModelEvent {
                    session_id: session_id.to_owned(),
                    activation_id: activation_id.to_owned(),
                    round_id: round_id.to_owned(),
                    text: chunk.to_owned(),
                }));
            remaining = rest;
        }
    }
}
