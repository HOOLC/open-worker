use std::sync::Arc;

use tokio::sync::Mutex;

use crate::config::RuntimeConfig;
use crate::db::GatewayDb;
use crate::jobs::JobSupervisor;

use crate::slack::{BotSelf, SlackGateway};
use zork_slack::AssistantStatusHub;

/// Admin-plane attachments; None only during early construction.
#[derive(Clone)]
pub struct AdminPlane {
    pub db: Arc<crate::control_db::ControlDb>,
    pub admin_token: Option<String>,
    pub started_at: String,
    pub ui_dir: std::path::PathBuf,
    pub reload_sock: std::path::PathBuf,
}

#[derive(Clone)]
pub struct AppState {
    pub config: RuntimeConfig,
    pub db: Arc<GatewayDb>,
    pub slack: SlackGateway,
    pub status: AssistantStatusHub,

    pub jobs: Arc<JobSupervisor>,
    pub bot: Arc<Mutex<Option<BotSelf>>>,
    pub admin: AdminPlane,
}
