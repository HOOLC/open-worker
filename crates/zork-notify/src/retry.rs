//! Failure-driven backoff. There is no periodic refresh while a feed is healthy.
use std::time::Duration;

pub struct Retry {
    initial: Duration,
    maximum: Duration,
    next: Duration,
}
impl Default for Retry {
    fn default() -> Self {
        Self::new(Duration::from_millis(750), Duration::from_secs(30))
    }
}
impl Retry {
    pub fn new(initial: Duration, maximum: Duration) -> Self {
        assert!(!initial.is_zero() && maximum >= initial);
        Self {
            initial,
            maximum,
            next: initial,
        }
    }
    /// Reset only after useful data or successful catch-up, not merely TCP open.
    pub fn reset(&mut self) {
        self.next = self.initial;
    }
    pub fn delay(&mut self) -> Duration {
        let delay = self.next;
        self.next = self.next.saturating_mul(2).min(self.maximum);
        delay
    }
    pub async fn wait(&mut self) {
        tokio::time::sleep(self.delay()).await;
    }
}
