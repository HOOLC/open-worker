//! Shared, level-triggered change notifications for the Agent Mesh.
//!
//! Notifications are hints, never business data or durable cursors. Subscribe
//! before reading the authority, checkpoint before each read, and persist any
//! replay cursor together with the data it covers. A slow observer retains a
//! pending change without retaining an unbounded queue of obsolete events.
mod blocking;
pub mod events;
pub mod files;
#[cfg(unix)]
pub mod io;
#[cfg(unix)]
pub mod process;
pub mod retry;
pub mod stream;

/// An owned background subscription. Removing it from a registry or dropping
/// its owner aborts the IO task instead of leaving a detached connection alive.
pub struct Task<T>(pub tokio::task::JoinHandle<T>);
impl<T> Task<T> {
    pub fn abort(&self) {
        self.0.abort();
    }
    pub fn is_finished(&self) -> bool {
        self.0.is_finished()
    }
}
impl<T> Drop for Task<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}
impl<T> std::future::Future for Task<T> {
    type Output = Result<T, tokio::task::JoinError>;
    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        std::pin::Pin::new(&mut self.0).poll(cx)
    }
}

use futures_util::future::select_all;
use std::{
    collections::{HashMap, HashSet},
    hash::Hash,
    sync::{Arc, Mutex},
};
use tokio::sync::watch;

/// A single change source. Publishing with no listeners does not allocate history.
#[derive(Clone)]
pub struct Notifier(watch::Sender<u64>);
impl Default for Notifier {
    fn default() -> Self {
        Self(watch::channel(0).0)
    }
}
impl Notifier {
    pub fn notify(&self) {
        self.0.send_modify(|v| *v = v.wrapping_add(1));
    }
    pub fn subscribe(&self) -> Changes {
        Changes {
            receivers: vec![self.0.subscribe()],
            registrations: vec![],
        }
    }
}

struct Registration<K> {
    topics: HashSet<K>,
    signal: Notifier,
}
struct Registry<K> {
    next: u64,
    listeners: HashMap<u64, Registration<K>>,
}

/// Exact topic routing. The publisher chooses its own typed domain/subject key;
/// a topic name alone never grants access to the underlying data.
pub struct Hub<K>(Arc<Mutex<Registry<K>>>);
impl<K> Clone for Hub<K> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<K> Default for Hub<K> {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(Registry {
            next: 0,
            listeners: HashMap::new(),
        })))
    }
}
impl<K: Eq + Hash + Send + 'static> Hub<K> {
    pub fn subscribe(&self, topics: impl IntoIterator<Item = K>) -> Changes {
        let mut registry = self.0.lock().expect("notification registry");
        let id = registry.next;
        registry.next = registry
            .next
            .checked_add(1)
            .expect("subscription id exhausted");
        let signal = Notifier::default();
        let mut changes = signal.subscribe();
        registry.listeners.insert(
            id,
            Registration {
                topics: topics.into_iter().collect(),
                signal,
            },
        );
        let weak = Arc::downgrade(&self.0);
        changes.registrations.push(Box::new(move || {
            if let Some(registry) = weak.upgrade() {
                registry
                    .lock()
                    .expect("notification registry")
                    .listeners
                    .remove(&id);
            }
        }));
        changes
    }
    /// A multi-topic commit wakes each matching listener at most once.
    pub fn publish(&self, topics: impl IntoIterator<Item = K>) {
        let topics: HashSet<_> = topics.into_iter().collect();
        if topics.is_empty() {
            return;
        }
        let registry = self.0.lock().expect("notification registry");
        for registration in registry.listeners.values() {
            if !registration.topics.is_disjoint(&topics) {
                registration.signal.notify();
            }
        }
    }
    pub fn listener_count(&self) -> usize {
        self.0
            .lock()
            .expect("notification registry")
            .listeners
            .len()
    }
}

/// A scoped subscription; dropping it unregisters its interests immediately.
/// It holds no strong reference to the publisher, so publisher shutdown closes it.
pub struct Changes {
    receivers: Vec<watch::Receiver<u64>>,
    registrations: Vec<Box<dyn FnOnce() + Send>>,
}
impl Drop for Changes {
    fn drop(&mut self) {
        for unregister in self.registrations.drain(..) {
            unregister();
        }
    }
}
impl Changes {
    /// Compose authorities (for example operation state and access policy)
    /// without forwarding tasks, timers, or another mutable business list.
    pub fn merge(mut self, mut other: Self) -> Self {
        self.receivers.append(&mut other.receivers);
        self.registrations.append(&mut other.registrations);
        self
    }
    /// Call before a read, never after it: changes during that read stay pending.
    pub fn checkpoint(&mut self) {
        for receiver in &mut self.receivers {
            receiver.borrow_and_update();
        }
    }
    pub async fn changed(&mut self) -> Result<(), watch::error::RecvError> {
        let (result, index) = {
            let (result, index, _) =
                select_all(self.receivers.iter_mut().map(|rx| Box::pin(rx.changed()))).await;
            (result, index)
        };
        let next = (index + 1) % self.receivers.len();
        self.receivers.rotate_left(next);
        result
    }
    /// Check once, then only when an authority changes. Useful for revocation,
    /// cancellation receipts and readiness conditions. Closure means unavailable.
    pub async fn until<T>(&mut self, mut check: impl FnMut() -> Option<T>) -> Option<T> {
        loop {
            self.checkpoint();
            if let Some(value) = check() {
                return Some(value);
            }
            self.changed().await.ok()?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::FutureExt;

    #[tokio::test]
    async fn topic_isolation_bursts_and_change_during_read() {
        let hub = Hub::default();
        let mut a = hub.subscribe(["a", "shared"]);
        let mut b = hub.subscribe(["b"]);
        a.checkpoint(); // before reading the authority
        for _ in 0..10_000 {
            hub.publish(["a", "shared"]);
        }
        a.changed().await.unwrap();
        assert!(a.changed().now_or_never().is_none());
        assert!(b.changed().now_or_never().is_none());
        hub.publish(["b"]);
        b.changed().await.unwrap();
        assert_eq!(hub.listener_count(), 2);
        drop(a);
        assert_eq!(hub.listener_count(), 1);
        drop(hub);
        assert!(b.changed().await.is_err());
    }

    #[tokio::test]
    async fn composed_sources_keep_concurrent_changes_and_close() {
        let a = Notifier::default();
        let b = Notifier::default();
        let mut changes = a.subscribe().merge(b.subscribe());
        a.notify();
        b.notify();
        changes.changed().await.unwrap();
        changes.checkpoint();
        assert!(changes.changed().now_or_never().is_none());
        drop(a);
        assert!(changes.changed().await.is_err());
    }

    #[tokio::test]
    async fn predicate_checks_initial_state_and_catches_racing_commit() {
        let notify = Notifier::default();
        let mut changes = notify.subscribe();
        let mut reads = 0;
        let value = changes
            .until(|| {
                reads += 1;
                if reads == 1 {
                    notify.notify();
                    None
                } else {
                    Some("committed")
                }
            })
            .await;
        assert_eq!(value, Some("committed"));
        assert_eq!(reads, 2);
    }
}
