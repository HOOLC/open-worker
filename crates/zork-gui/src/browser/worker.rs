//! GPUI lifetime hook around the core browser host.
#[derive(Clone)]
pub struct Worker(pub zork_client_core::desktop::browser_worker::Worker);
impl gpui::Global for Worker {}
impl std::ops::Deref for Worker {
    type Target = zork_client_core::desktop::browser_worker::Worker;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl Worker {
    pub fn new() -> Self {
        Self(zork_client_core::desktop::browser_worker::Worker::new())
    }
    pub fn install_shutdown(&self, cx: &gpui::App) {
        let worker = self.0.clone();
        cx.on_app_quit(move |cx| {
            let worker = worker.clone();
            let close = cx.background_executor().spawn(async move {
                worker.shutdown();
            });
            async move {
                close.await;
            }
        })
        .detach();
    }
}
