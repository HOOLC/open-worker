#[derive(Clone, Debug)]
pub struct TimerArm {
    pub session_id: String,
    pub wait_id: String,
    pub deadline_ms: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TimerKey {
    pub session_id: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimerPortError {
    Shutdown,
    Backend,
}

pub trait TimerPort: Send + Sync {
    fn arm(&self, arm: TimerArm) -> Result<(), TimerPortError>;
    fn cancel(&self, key: &TimerKey) -> Result<(), TimerPortError>;
    fn shutdown(&self);
}
