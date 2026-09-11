//! The story host supplies only fixture input and execution/clock capabilities.
use gpui::App;
use std::{future::Future, pin::Pin, sync::Arc, time::Duration};
pub use zork_client_core::agent_edit::{
    compatible_profiles, profile_options, repair_profile, validate_selection, AgentInput,
};
pub use zork_client_core::api::*;
pub use zork_client_core::model_edit::{
    compact_tokens, model_accepts_images, ConnectionInput, ModelInput, MODEL_APIS,
};
pub use zork_client_core::state::{
    AgentData, AgentUpdate, Agents, ProfileData, ProfileUpdate, Profiles,
};
struct StoryExecutor {
    foreground: gpui::ForegroundExecutor,
    background: gpui::BackgroundExecutor,
}
impl Executor for StoryExecutor {
    fn wait(&self, duration: Duration) -> Pin<Box<dyn Future<Output = ()> + 'static>> {
        Box::pin(self.background.timer(duration))
    }
    fn spawn(&self, future: Pin<Box<dyn Future<Output = ()> + 'static>>) {
        self.foreground.spawn(future).detach();
    }
}
pub fn initialize(cx: &App) {
    configure_fixture(
        zork_ui::stories::page_fixture().clone(),
        serde_json::from_str(include_str!(
            "../../zork-gui/tests/fixtures/provider_catalog.json"
        ))
        .expect("provider fixture"),
        Arc::new(StoryExecutor {
            foreground: cx.foreground_executor().clone(),
            background: cx.background_executor().clone(),
        }),
    );
}

pub use zork_client_core::agent_edit::thinking_after_choice;
pub use zork_client_core::model_edit::{copy_form, copyable, model_form};

pub use zork_client_core::model_edit::connection_options;
