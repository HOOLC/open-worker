use std::{future::Future, pin::Pin, time::Duration};

pub trait Clock: Send + Sync {
    fn now_ms(&self) -> i64;
    fn sleep(&self, duration: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + 'static>>;
}
