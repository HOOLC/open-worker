//! One subscription lifecycle for HTTP, Mesh and in-process sources.
use crate::Changes;
use std::{future::Future, time::Duration};
use tokio::sync::mpsc;

pub enum Event<T, E> {
    Data(T),
    Heartbeat,
    Error(E),
}

/// A snapshot coalesces; journal pages drain immediately until caught up.
pub struct Page<T> {
    pub value: T,
    pub more: bool,
    pub done: bool,
    pub deduplicate: bool,
}
impl<T> Page<T> {
    pub fn snapshot(value: T) -> Self {
        Self {
            value,
            more: false,
            done: false,
            deduplicate: true,
        }
    }
    pub fn chunk(value: T, more: bool, done: bool) -> Self {
        Self {
            value,
            more,
            done,
            deduplicate: false,
        }
    }
}

/// A domain provides reads and authorization; the driver owns notification
/// ordering, batching, backpressure, heartbeat and subscriber cancellation.
pub trait Source: Send {
    type Item: Clone + PartialEq + Send;
    type Error: Send;

    /// Check current access while output is full, without querying business data.
    /// Restricted sources implement this or use a surrounding `guard`.
    fn check_access(&self) -> Result<(), Self::Error> {
        Ok(())
    }
    /// A capability can expire even when neither data nor policy changes.
    /// This is one absolute deadline, enforced during reads and backpressure.
    fn expiry(&self) -> Option<(tokio::time::Instant, Self::Error)> {
        None
    }

    /// Recheck access on every read. Do not advance a cursor here.
    fn read(&mut self) -> impl Future<Output = Result<Page<Self::Item>, Self::Error>> + Send;
    /// Advance an outgoing stream cursor only after the page entered the queue.
    /// The consumer still persists its own cursor after applying the page.
    fn delivered(&mut self, _value: &Self::Item) {}
}

pub async fn serve<S: Source>(
    mut source: S,
    mut changes: Changes,
    tx: mpsc::Sender<Event<S::Item, S::Error>>,
    heartbeat: Option<Duration>,
) {
    assert!(heartbeat.is_none_or(|delay| !delay.is_zero()));
    let expiry = source.expiry();
    let deadline = expiry.as_ref().map(|(at, _)| *at);
    let mut expiry_error = expiry.map(|(_, error)| error);
    let mut previous = None;
    'read: loop {
        // Acquire capacity before reading. Continuous updates cannot starve
        // delivery or make a slow observer repeatedly query business state.
        let permit = loop {
            tokio::select! {
                biased;
                _ = tx.closed() => return,
                _ = until(deadline) => {
                    if let Some(error) = expiry_error.take() { let _ = tx.try_send(Event::Error(error)); }
                    return;
                },
                permit = tx.reserve() => match permit { Ok(permit) => break permit, Err(_) => return },
                changed = changes.changed() => {
                    if changed.is_err() { return; }
                    if let Err(error) = source.check_access() {
                        let _ = tx.try_send(Event::Error(error));
                        return;
                    }
                }
            }
        };
        changes.checkpoint();
        let page = tokio::select! {
            _ = tx.closed() => return,
            _ = until(deadline) => {
                if let Some(error) = expiry_error.take() { permit.send(Event::Error(error)); }
                return;
            },
            page = source.read() => page,
        };
        let page = match page {
            Ok(page) => page,
            Err(error) => {
                permit.send(Event::Error(error));
                return;
            }
        };
        if !page.deduplicate || previous.as_ref() != Some(&page.value) || page.done {
            source.delivered(&page.value);
            previous = Some(page.value.clone());
            permit.send(Event::Data(page.value));
        } else {
            drop(permit);
        }
        if page.done {
            return;
        }
        if page.more {
            continue;
        }
        loop {
            tokio::select! {
                biased;
                _ = tx.closed() => return,
                _ = until(deadline) => {
                    if let Some(error) = expiry_error.take() { let _ = tx.try_send(Event::Error(error)); }
                    return;
                },
                changed = changes.changed() => {
                    if changed.is_err() { return; }
                    continue 'read;
                }
                _ = async {
                    match heartbeat {
                        Some(delay) => tokio::time::sleep(delay).await,
                        None => std::future::pending().await,
                    }
                } => {
                    // Heartbeats never re-read business state. A full queue
                    // already has data to send; do not enqueue stale keepalives.
                    let _ = tx.try_send(Event::Heartbeat);
                }
            }
        }
    }
}

async fn until(deadline: Option<tokio::time::Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

/// Adapt the same driver to a transport's frames. Both bounded queues and the
/// read task are released when the final receiver closes; no detached forwarder.
pub fn spawn<S, F, O>(
    source: S,
    changes: Changes,
    capacity: usize,
    heartbeat: Option<Duration>,
    mut encode: F,
) -> mpsc::Receiver<O>
where
    S: Source + 'static,
    S::Item: 'static,
    S::Error: 'static,
    F: FnMut(Event<S::Item, S::Error>) -> Option<O> + Send + 'static,
    O: Send + 'static,
{
    let (tx, rx) = mpsc::channel(capacity);
    let (events, mut input) = mpsc::channel(1);
    tokio::spawn(async move {
        let driver = serve(source, changes, events, heartbeat);
        let forward = async move {
            loop {
                let event = tokio::select! {
                    _ = tx.closed() => return,
                    event = input.recv() => match event { Some(event) => event, None => return },
                };
                if let Some(frame) = encode(event) {
                    if tx.send(frame).await.is_err() {
                        return;
                    }
                }
            }
        };
        tokio::join!(driver, forward);
    });
    rx
}

/// Compose independent projections and live events into one owned connection.
/// Closing any input requires resubscription/catch-up; do not silently leave a
/// partially live business view behind. Closing output releases all inputs.
pub fn merge<T: Send + 'static>(
    mut inputs: Vec<mpsc::Receiver<T>>,
    capacity: usize,
) -> mpsc::Receiver<T> {
    assert!(!inputs.is_empty());
    let (tx, rx) = mpsc::channel(capacity);
    tokio::spawn(async move {
        loop {
            let (value, index) = tokio::select! {
                _ = tx.closed() => return,
                value = async {
                    let (value, index, _) = futures_util::future::select_all(inputs.iter_mut().map(|rx| Box::pin(rx.recv()))).await;
                    (value, index)
                } => match value { (Some(value), index) => (value, index), (None, _) => return },
            };
            let next = (index + 1) % inputs.len();
            inputs.rotate_left(next);
            if tx.send(value).await.is_err() {
                return;
            }
        }
    });
    rx
}

/// Revoke a transport subscription on a policy change, even when output is
/// backpressured. The permission closure must read the current authority.
pub fn guard<T, F>(
    mut input: mpsc::Receiver<T>,
    mut policy: Changes,
    allowed: F,
    capacity: usize,
) -> mpsc::Receiver<T>
where
    T: Send + 'static,
    F: Fn() -> bool + Send + Sync + 'static,
{
    let (tx, rx) = mpsc::channel(capacity);
    tokio::spawn(async move {
        let denied = policy.until(|| (!allowed()).then_some(()));
        tokio::pin!(denied);
        loop {
            let value = tokio::select! {
                biased;
                _ = &mut denied => return,
                _ = tx.closed() => return,
                value = input.recv() => match value { Some(value) => value, None => return },
            };
            tokio::select! {
                biased;
                _ = &mut denied => return,
                result = tx.send(value) => if result.is_err() { return; },
            }
        }
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Hub;
    use futures_util::FutureExt;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct Authority {
        value: u64,
        reads: u64,
        delivered: Vec<u64>,
        revoked: bool,
    }
    struct Snapshot(Arc<Mutex<Authority>>);
    struct Expiring {
        snapshot: Snapshot,
        at: tokio::time::Instant,
        blocked_read: bool,
    }
    impl Source for Expiring {
        type Item = u64;
        type Error = &'static str;
        fn expiry(&self) -> Option<(tokio::time::Instant, Self::Error)> {
            Some((self.at, "expired"))
        }
        async fn read(&mut self) -> Result<Page<u64>, Self::Error> {
            if self.blocked_read {
                std::future::pending().await
            } else {
                self.snapshot.read().await
            }
        }
    }

    #[tokio::test(start_paused = true)]
    async fn capability_expiry_interrupts_idle_full_output_and_inflight_reads() {
        for mode in 0..3 {
            let hub = Hub::default();
            let authority = Arc::new(Mutex::new(Authority::default()));
            let (tx, mut rx) = mpsc::channel(1);
            if mode == 1 {
                assert!(tx.send(Event::Heartbeat).await.is_ok());
            }
            let source = Expiring {
                snapshot: Snapshot(authority.clone()),
                at: tokio::time::Instant::now() + Duration::from_secs(10),
                blocked_read: mode == 2,
            };
            let task = tokio::spawn(serve(source, hub.subscribe(["claim"]), tx, None));
            if mode == 0 {
                assert!(matches!(rx.recv().await, Some(Event::Data(0))));
            }
            tokio::time::advance(Duration::from_secs(10)).await;
            task.await.unwrap();
            assert_eq!(
                authority.lock().unwrap().reads,
                if mode == 0 { 1 } else { 0 }
            );
            if mode == 1 {
                assert!(matches!(rx.recv().await, Some(Event::Heartbeat)));
            } else {
                assert!(matches!(rx.recv().await, Some(Event::Error("expired"))));
            }
            assert!(rx.recv().await.is_none());
            assert_eq!(hub.listener_count(), 0);
        }
    }
    impl Source for Snapshot {
        type Item = u64;
        type Error = &'static str;
        fn check_access(&self) -> Result<(), &'static str> {
            if self.0.lock().unwrap().revoked {
                Err("revoked")
            } else {
                Ok(())
            }
        }
        async fn read(&mut self) -> Result<Page<u64>, &'static str> {
            let mut authority = self.0.lock().unwrap();
            authority.reads += 1;
            if authority.revoked {
                return Err("revoked");
            }
            Ok(Page::snapshot(authority.value))
        }
        fn delivered(&mut self, value: &u64) {
            self.0.lock().unwrap().delivered.push(*value);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn healthy_feed_has_no_business_reads_on_heartbeats_and_coalesces_bursts() {
        let hub = Hub::default();
        let authority = Arc::new(Mutex::new(Authority::default()));
        let (tx, mut rx) = mpsc::channel(4);
        let task = tokio::spawn(serve(
            Snapshot(authority.clone()),
            hub.subscribe(["catalog"]),
            tx,
            Some(Duration::from_secs(10)),
        ));
        assert!(matches!(rx.recv().await, Some(Event::Data(0))));
        for _ in 0..12 {
            tokio::time::advance(Duration::from_secs(10)).await;
            assert!(matches!(rx.recv().await, Some(Event::Heartbeat)));
        }
        assert_eq!(authority.lock().unwrap().reads, 1);
        for value in 1..=1000 {
            authority.lock().unwrap().value = value;
            hub.publish(["catalog"]);
        }
        assert!(matches!(rx.recv().await, Some(Event::Data(1000))));
        assert_eq!(authority.lock().unwrap().reads, 2);
        hub.publish(["unrelated"]);
        assert!(rx.recv().now_or_never().is_none());
        drop(rx);
        task.await.unwrap();
        assert_eq!(hub.listener_count(), 0);
    }

    #[tokio::test]
    async fn revocation_interrupts_backpressure_without_advancing_unsent_cursor() {
        let hub = Hub::default();
        let authority = Arc::new(Mutex::new(Authority::default()));
        let (tx, mut rx) = mpsc::channel(1);
        let task = tokio::spawn(serve(
            Snapshot(authority.clone()),
            hub.subscribe(["data", "policy"]),
            tx,
            None,
        ));
        tokio::task::yield_now().await;
        for value in 1..=1000 {
            authority.lock().unwrap().value = value;
            hub.publish(["data"]);
        }
        tokio::task::yield_now().await;
        assert_eq!(authority.lock().unwrap().reads, 1);
        authority.lock().unwrap().revoked = true;
        hub.publish(["policy"]);
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(authority.lock().unwrap().delivered, vec![0]);
        assert!(matches!(rx.recv().await, Some(Event::Data(0))));
        assert!(rx.recv().await.is_none());
        assert_eq!(hub.listener_count(), 0);
    }

    struct Journal {
        after: u64,
        through: u64,
    }
    impl Source for Journal {
        type Item = u64;
        type Error = &'static str;
        async fn read(&mut self) -> Result<Page<u64>, &'static str> {
            let next = self.after + 1;
            Ok(Page::chunk(next, next < self.through, next == self.through))
        }
        fn delivered(&mut self, value: &u64) {
            self.after = *value;
        }
    }
    #[tokio::test]
    async fn journal_drains_without_further_notifications_and_resumes_committed_cursor() {
        let hub = Hub::default();
        for after in [0, 7] {
            let mut rx = spawn(
                Journal { after, through: 20 },
                hub.subscribe(["journal"]),
                1,
                None,
                |event| match event {
                    Event::Data(value) => Some(value),
                    _ => None,
                },
            );
            let mut values = vec![];
            while let Some(value) = rx.recv().await {
                values.push(value);
            }
            assert_eq!(values, (after + 1..=20).collect::<Vec<_>>());
        }
        assert_eq!(hub.listener_count(), 0);
    }

    #[tokio::test]
    async fn subscriber_drop_cancels_an_inflight_read() {
        struct Blocked(tokio::sync::oneshot::Sender<()>);
        impl Source for Blocked {
            type Item = u64;
            type Error = ();
            async fn read(&mut self) -> Result<Page<u64>, ()> {
                let _ = &self.0;
                std::future::pending().await
            }
        }
        let hub = Hub::default();
        let (alive, dropped) = tokio::sync::oneshot::channel();
        let rx = spawn(Blocked(alive), hub.subscribe(["slow"]), 1, None, |_| {
            Some(())
        });
        tokio::task::yield_now().await;
        drop(rx);
        assert!(tokio::time::timeout(Duration::from_secs(1), dropped)
            .await
            .unwrap()
            .is_err());
        assert_eq!(hub.listener_count(), 0);
    }

    #[tokio::test]
    async fn policy_guard_releases_blocked_input_and_merge_closes_all_sources() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let policy = Hub::default();
        let allowed = Arc::new(AtomicBool::new(true));
        let current = allowed.clone();
        let (source, input) = mpsc::channel(4);
        let mut output = guard(
            input,
            policy.subscribe(["grant"]),
            move || current.load(Ordering::SeqCst),
            1,
        );
        source.send(1).await.unwrap();
        source.send(2).await.unwrap();
        tokio::task::yield_now().await;
        allowed.store(false, Ordering::SeqCst);
        policy.publish(["grant"]);
        tokio::time::timeout(Duration::from_secs(1), source.closed())
            .await
            .unwrap();
        assert_eq!(output.recv().await, Some(1));
        assert_eq!(output.recv().await, None);
        let (a, first) = mpsc::channel::<u64>(1);
        let (b, second) = mpsc::channel::<u64>(1);
        let mut merged = merge(vec![first, second], 1);
        drop(a);
        assert_eq!(merged.recv().await, None);
        b.closed().await;
    }

    #[tokio::test]
    async fn busy_projection_cannot_starve_another_sources_message() {
        let (busy, first) = mpsc::channel(4);
        let (message, second) = mpsc::channel(1);
        for value in 0..4 {
            busy.send(value).await.unwrap();
        }
        message.send(99).await.unwrap();
        let mut output = merge(vec![first, second], 1);
        assert_eq!(output.recv().await, Some(0));
        assert_eq!(output.recv().await, Some(99));
        drop(output);
        busy.closed().await;
        message.closed().await;
    }

    #[tokio::test]
    async fn a_commit_during_every_read_cannot_starve_delivery() {
        use std::sync::atomic::{AtomicU64, Ordering};
        struct Racing {
            value: Arc<AtomicU64>,
            updates: crate::Notifier,
        }
        impl Source for Racing {
            type Item = u64;
            type Error = ();
            async fn read(&mut self) -> Result<Page<u64>, ()> {
                let snapshot = self.value.load(Ordering::SeqCst);
                // Deterministically place an independent writer between the
                // authority read and publication, on every iteration.
                let writer = self.value.clone();
                let updates = self.updates.clone();
                tokio::spawn(async move {
                    writer.fetch_add(1, Ordering::SeqCst);
                    updates.notify();
                })
                .await
                .unwrap();
                Ok(Page::snapshot(snapshot))
            }
        }
        let updates = crate::Notifier::default();
        let changes = updates.subscribe();
        let (tx, mut rx) = mpsc::channel(1);
        let task = tokio::spawn(serve(
            Racing {
                value: Default::default(),
                updates,
            },
            changes,
            tx,
            None,
        ));
        tokio::time::timeout(Duration::from_secs(1), async {
            for expected in 0..10 {
                assert!(matches!(rx.recv().await, Some(Event::Data(value)) if value == expected));
            }
        })
        .await
        .unwrap();
        drop(rx);
        task.await.unwrap();
    }
}
