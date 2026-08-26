use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use tokio::{sync::mpsc::UnboundedSender, task::JoinHandle};

use crate::session::runtime::ports::{Clock, TimerArm, TimerKey, TimerPort, TimerPortError};

struct ArmedWait {
    handle: JoinHandle<()>,
    generation: u64,
}

struct SleepTimerInner {
    due_tx: Option<UnboundedSender<TimerArm>>,
    arms: HashMap<String, ArmedWait>,
    next_generation: u64,
}

pub struct SleepTimer {
    clock: Arc<dyn Clock>,
    inner: Arc<Mutex<SleepTimerInner>>,
    shutdown: AtomicBool,
}

impl SleepTimer {
    pub fn new(clock: Arc<dyn Clock>, due_tx: UnboundedSender<TimerArm>) -> Self {
        Self {
            clock,
            inner: Arc::new(Mutex::new(SleepTimerInner {
                due_tx: Some(due_tx),
                arms: HashMap::new(),
                next_generation: 0,
            })),
            shutdown: AtomicBool::new(false),
        }
    }
}

impl TimerPort for SleepTimer {
    fn arm(&self, arm: TimerArm) -> Result<(), TimerPortError> {
        if self.shutdown.load(Ordering::SeqCst) {
            return Err(TimerPortError::Shutdown);
        }
        let delay_ms = arm.deadline_ms.saturating_sub(self.clock.now_ms());
        let delay = Duration::from_millis(u64::try_from(delay_ms).unwrap_or(0));
        let clock = self.clock.clone();
        let mut inner = self.inner.lock().map_err(|_| TimerPortError::Backend)?;
        let Some(tx) = inner.due_tx.clone() else {
            return Err(TimerPortError::Shutdown);
        };
        if self.shutdown.load(Ordering::SeqCst) {
            return Err(TimerPortError::Shutdown);
        }
        let session_id = arm.session_id.clone();
        let generation = inner.next_generation;
        inner.next_generation = inner.next_generation.wrapping_add(1);
        let cleanup = self.inner.clone();
        let cleanup_session = session_id.clone();
        let handle = tokio::spawn(async move {
            clock.sleep(delay).await;
            let _ = tx.send(arm);
            if let Ok(mut guard) = cleanup.lock() {
                if guard
                    .arms
                    .get(&cleanup_session)
                    .is_some_and(|armed| armed.generation == generation)
                {
                    guard.arms.remove(&cleanup_session);
                }
            }
        });
        if let Some(previous) = inner
            .arms
            .insert(session_id, ArmedWait { handle, generation })
        {
            previous.handle.abort();
        }
        Ok(())
    }

    fn cancel(&self, key: &TimerKey) -> Result<(), TimerPortError> {
        let mut inner = self.inner.lock().map_err(|_| TimerPortError::Backend)?;
        if let Some(previous) = inner.arms.remove(&key.session_id) {
            previous.handle.abort();
        }
        Ok(())
    }

    fn shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
        if let Ok(mut inner) = self.inner.lock() {
            inner.due_tx.take();
            for (_, armed) in inner.arms.drain() {
                armed.handle.abort();
            }
        }
    }
}
