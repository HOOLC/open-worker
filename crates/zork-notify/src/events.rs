//! Bounded transient event fan-out. Lag is explicit and must be repaired from
//! the domain's durable journal; publishing without observers retains no data.
use std::{
    borrow::Borrow,
    collections::HashMap,
    hash::Hash,
    sync::{Arc, Mutex},
};
use tokio::sync::broadcast;

struct Topic<T> {
    sender: broadcast::Sender<T>,
    listeners: usize,
}
pub struct EventHub<K, T> {
    capacity: usize,
    topics: Arc<Mutex<HashMap<K, Topic<T>>>>,
}
impl<K, T> Clone for EventHub<K, T> {
    fn clone(&self) -> Self {
        Self {
            capacity: self.capacity,
            topics: self.topics.clone(),
        }
    }
}
impl<K: Clone + Eq + Hash + Send + 'static, T: Clone + Send + 'static> EventHub<K, T> {
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0);
        Self {
            capacity,
            topics: Default::default(),
        }
    }
    pub fn subscribe(&self, key: K) -> Events<T> {
        let mut topics = self.topics.lock().expect("event topics");
        let topic = topics.entry(key.clone()).or_insert_with(|| Topic {
            sender: broadcast::channel(self.capacity).0,
            listeners: 0,
        });
        topic.listeners += 1;
        let receiver = topic.sender.subscribe();
        let weak = Arc::downgrade(&self.topics);
        Events {
            receiver,
            unregister: Some(Box::new(move || {
                if let Some(topics) = weak.upgrade() {
                    let mut topics = topics.lock().expect("event topics");
                    if let Some(topic) = topics.get_mut(&key) {
                        topic.listeners -= 1;
                        if topic.listeners == 0 {
                            topics.remove(&key);
                        }
                    }
                }
            })),
        }
    }
    /// Construct potentially expensive payloads only when somebody observes.
    pub fn publish_with<Q: Hash + Eq + ?Sized>(&self, key: &Q, value: impl FnOnce() -> T)
    where
        K: Borrow<Q>,
    {
        if let Some(topic) = self.topics.lock().expect("event topics").get(key) {
            let _ = topic.sender.send(value());
        }
    }
    pub fn topic_count(&self) -> usize {
        self.topics.lock().expect("event topics").len()
    }
}

pub struct Events<T> {
    receiver: broadcast::Receiver<T>,
    unregister: Option<Box<dyn FnOnce() + Send>>,
}
impl<T> Drop for Events<T> {
    fn drop(&mut self) {
        if let Some(unregister) = self.unregister.take() {
            unregister();
        }
    }
}
impl<T: Clone> Events<T> {
    pub async fn recv(&mut self) -> Result<T, broadcast::error::RecvError> {
        self.receiver.recv().await
    }
    pub fn try_recv(&mut self) -> Result<T, broadcast::error::TryRecvError> {
        self.receiver.try_recv()
    }
}

impl<T: Clone + Send + 'static> Events<T> {
    /// Attach transport encoding and optional initial state to a transient feed.
    /// Domains explicitly map Lagged to their journal/snapshot recovery hint.
    pub fn forward<O, F>(
        mut self,
        capacity: usize,
        initial: Option<O>,
        mut encode: F,
    ) -> tokio::sync::mpsc::Receiver<O>
    where
        O: Send + 'static,
        F: FnMut(Result<T, broadcast::error::RecvError>) -> Option<O> + Send + 'static,
    {
        let (tx, rx) = tokio::sync::mpsc::channel(capacity);
        tokio::spawn(async move {
            if let Some(initial) = initial {
                if tx.send(initial).await.is_err() {
                    return;
                }
            }
            loop {
                let event = tokio::select! {
                    _ = tx.closed() => return,
                    event = self.recv() => event,
                };
                if matches!(event, Err(broadcast::error::RecvError::Closed)) {
                    return;
                }
                if let Some(value) = encode(event) {
                    if tx.send(value).await.is_err() {
                        return;
                    }
                }
            }
        });
        rx
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn lag_is_explicit_topics_are_scoped_and_publishers_can_close() {
        let hub = EventHub::<String, u64>::new(2);
        hub.publish_with("absent", || panic!("unobserved payload constructed"));
        let mut a = hub.subscribe("a".into());
        let mut b = hub.subscribe("b".into());
        for n in 0..10 {
            hub.publish_with("a", || n);
        }
        assert!(matches!(
            a.recv().await,
            Err(broadcast::error::RecvError::Lagged(8))
        ));
        assert_eq!(a.recv().await.unwrap(), 8);
        assert_eq!(a.recv().await.unwrap(), 9);
        assert!(matches!(
            b.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
        drop(a);
        assert_eq!(hub.topic_count(), 1);
        drop(hub);
        assert!(matches!(
            b.recv().await,
            Err(broadcast::error::RecvError::Closed)
        ));
    }
}
