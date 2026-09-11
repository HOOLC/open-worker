// Reuse the native geometry recorder and physical-input dispatcher; no HTTP server.
pub use element::{AutomationElementExt, AutomationRoot};
use element::{AutomationRegistry, AutomationRegistryGlobal};
pub use protocol::AutomationRole;
pub use zork_ui::automation::driver;
pub use zork_ui::automation::element;
pub use zork_ui::automation::protocol;
#[derive(Clone)]
pub struct Automation {
    registry: AutomationRegistry,
}
impl Automation {
    pub fn install(cx: &mut gpui::App) -> Self {
        let registry = AutomationRegistry::new();
        cx.set_global(AutomationRegistryGlobal(registry.clone()));
        Self { registry }
    }
    pub fn snapshot(&self) -> protocol::UiSnapshot {
        self.registry.snapshot(false)
    }
    pub fn dispatch(
        &self,
        action: protocol::UserAction,
        w: &mut gpui::Window,
        cx: &mut gpui::App,
    ) -> anyhow::Result<()> {
        driver::headless_action(action, w, cx, &self.registry)
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!("{}: {}", e.code, e.message))
    }
}
