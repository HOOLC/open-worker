//! Blocking host adapters use the same readiness future, never a sampling clock.
use std::{
    future::Future,
    sync::{Arc, Condvar, Mutex},
    task::{Context, Poll, Wake, Waker},
    time::Instant,
};

#[derive(Default)]
struct Signal(Mutex<bool>, Condvar);
impl Wake for Signal {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        *self.0.lock().unwrap() = true;
        self.1.notify_one();
    }
}

impl crate::Changes {
    /// `false` means the one absolute deadline expired. Call `checkpoint`
    /// before reading, just as for async consumers. This needs no Tokio runtime.
    pub fn blocking_changed(
        &mut self,
        deadline: Instant,
    ) -> Result<bool, tokio::sync::watch::error::RecvError> {
        let signal = Arc::new(Signal::default());
        let waker = Waker::from(signal.clone());
        let mut context = Context::from_waker(&waker);
        let mut change = std::pin::pin!(self.changed());
        loop {
            if let Poll::Ready(result) = change.as_mut().poll(&mut context) {
                return result.map(|()| true);
            }
            let mut awake = signal.0.lock().unwrap();
            while !*awake {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Ok(false);
                }
                awake = signal.1.wait_timeout(awake, remaining).unwrap().0;
            }
            *awake = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn blocking_adapter_observes_pending_changes_and_has_one_deadline() {
        let source = crate::Notifier::default();
        let mut changes = source.subscribe();
        source.notify();
        assert!(changes
            .blocking_changed(Instant::now() + Duration::from_secs(1))
            .unwrap());
        assert!(!changes
            .blocking_changed(Instant::now() + Duration::from_millis(10))
            .unwrap());
        let sender = source.clone();
        let worker = std::thread::spawn(move || sender.notify());
        assert!(changes
            .blocking_changed(Instant::now() + Duration::from_secs(1))
            .unwrap());
        worker.join().unwrap();
        drop(source);
        assert!(changes
            .blocking_changed(Instant::now() + Duration::from_secs(1))
            .is_err());
    }
}
