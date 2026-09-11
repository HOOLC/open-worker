//! Latest-value projections over the version/acknowledgement protocol.
use crate::{Batch, BatchId, Cursor, JournalLimits, Readiness, Source, Subscription, Topics};
use std::sync::{Arc, Mutex};

pub struct ValueSource<T> {
    source: Source<T>,
    publishing: Mutex<()>,
}

impl<T> ValueSource<T> {
    pub fn new(value: T) -> Self {
        Self {
            source: Source::new(value, JournalLimits::default()),
            publishing: Mutex::new(()),
        }
    }
    pub fn read(&self) -> Arc<T> {
        self.source.snapshot().value
    }
    pub fn observed(&self) -> bool {
        self.source.observed()
    }
    pub fn subscribe(&self) -> ValueSubscription<T> {
        self.subscribe_topics(Topics::ALL)
    }
    pub fn subscribe_topics(&self, topics: Topics) -> ValueSubscription<T> {
        let (source, opening) = self.source.subscribe_topics(topics);
        ValueSubscription {
            source,
            opening: opening.cursor,
            initial: true,
        }
    }
    pub fn publish_changed(&self, value: T, topics: Topics) {
        let _publishing = self.publishing.lock().expect("value publication");
        self.source.publish(value, (), topics, 0);
    }
    pub fn invalidate(&self, value: T) {
        let _publishing = self.publishing.lock().expect("value publication");
        self.source.invalidate(value);
    }
}

impl<T: PartialEq> ValueSource<T> {
    /// Compatibility helper for small sources. Explicit reducers use
    /// publish_changed, avoiding deep comparison of an entire business state.
    pub fn publish(&self, value: T) -> bool {
        let _publishing = self.publishing.lock().expect("value publication");
        if *self.source.snapshot().value == value {
            return false;
        }
        self.source.publish(value, (), Topics::ALL, 0);
        true
    }
}

pub struct ValueSubscription<T> {
    source: Subscription<T>,
    opening: Cursor,
    initial: bool,
}

impl<T> ValueSubscription<T> {
    pub fn current(&self) -> crate::Snapshot<T> {
        self.source.snapshot()
    }
    pub fn readiness(&self) -> Readiness {
        self.source.readiness()
    }
    pub async fn ready(&mut self) -> Result<(), crate::Closed> {
        self.source.ready().await
    }
    pub fn prepare(&mut self) -> Option<Arc<Batch<T, ()>>> {
        self.source.prepare()
    }
    pub fn acknowledge(&mut self, batch: BatchId) -> bool {
        let applied = self.source.acknowledge(batch);
        if applied {
            self.initial = false;
        }
        applied
    }
    pub fn discard(&mut self, batch: BatchId) -> bool {
        self.source.discard(batch)
    }
    pub fn valid(&self, batch: BatchId) -> bool {
        self.source.valid(batch)
    }
    pub fn reset(&mut self) {
        self.source.reset();
    }

    /// Synchronous compatibility read. Platforms with a cancellable handoff
    /// use prepare/acknowledge to retain their last applied baseline.
    pub fn snapshot(&mut self) -> Arc<T> {
        if let Some(batch) = self.prepare() {
            let value = batch.snapshot.value.clone();
            self.acknowledge(batch.id);
            value
        } else {
            self.source.snapshot().value
        }
    }

    pub async fn changed(&mut self) -> Option<Arc<T>> {
        loop {
            self.ready().await.ok()?;
            if let Some(batch) = self.prepare() {
                // Preserve watch's changed-before-first-snapshot semantics.
                let only_initial = self.initial && batch.snapshot.cursor == self.opening;
                let value = batch.snapshot.value.clone();
                self.acknowledge(batch.id);
                if !only_initial {
                    return Some(value);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::FutureExt;

    #[test]
    fn compatibility_waits_are_quiet_on_open_and_noops_while_prepare_is_explicit() {
        let source = ValueSource::new(7);
        let mut legacy = source.subscribe();
        let mut explicit = source.subscribe();
        assert!(legacy.changed().now_or_never().is_none());
        assert!(explicit.prepare().unwrap().is_reset());
        assert!(!source.publish(7));
        assert!(legacy.changed().now_or_never().is_none());
        assert!(source.publish(8));
        assert_eq!(*legacy.changed().now_or_never().unwrap().unwrap(), 8);
        assert!(legacy.changed().now_or_never().is_none());
        drop(source);
        assert!(legacy.changed().now_or_never().unwrap().is_none());
    }

    #[test]
    fn publication_before_the_first_wait_is_not_mistaken_for_the_initial_value() {
        let source = ValueSource::new(0);
        let mut receiver = source.subscribe();
        source.publish(1);
        assert_eq!(*receiver.changed().now_or_never().unwrap().unwrap(), 1);
    }
}
