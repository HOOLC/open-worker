use std::env;
use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Context, Result};

#[derive(Clone, Debug)]
pub struct GatewayConfig {
    pub process_id: String,
    pub state_dir: PathBuf,
    pub bind_addr: SocketAddr,
    pub slack_app_token: String,
    pub slack_bot_token: String,
    pub slack_api_base_url: String,
    pub slack_socket_open_path: String,
    pub lease_ttl_ms: i64,
}

impl GatewayConfig {
    pub fn from_env() -> Result<Self> {
        let port: u16 = env_or("PORT", "3000").parse().context("PORT")?;
        let host = env_or("GATEWAY_BIND_HOST", "127.0.0.1");
        let bind_addr = format!("{host}:{port}")
            .parse()
            .context("gateway bind addr")?;
        let data_root = env::var("DATA_ROOT")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(default_data_root);
        let state_dir = env::var("STATE_DIR")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| data_root.join("state"));
        Ok(Self {
            process_id: format!(
                "gateway-{}-{}",
                std::process::id(),
                &uuid::Uuid::new_v4().to_string()[..8]
            ),
            state_dir,
            bind_addr,
            slack_app_token: required("SLACK_APP_TOKEN")?,
            slack_bot_token: required("SLACK_BOT_TOKEN")?,
            slack_api_base_url: env_or("SLACK_API_BASE_URL", "https://slack.com/api")
                .trim_end_matches('/')
                .to_string(),
            slack_socket_open_path: env_or("SLACK_SOCKET_OPEN_URL", "apps.connections.open"),
            lease_ttl_ms: env_or("SPOOL_LEASE_MS", "30000")
                .parse()
                .context("SPOOL_LEASE_MS")?,
        })
    }
}

fn required(key: &str) -> Result<String> {
    env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .with_context(|| format!("missing required environment variable: {key}"))
}

fn env_or(key: &str, default: &str) -> String {
    env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| default.to_string())
}

fn default_data_root() -> PathBuf {
    home_dir().join(".zork")
}

fn home_dir() -> PathBuf {
    env::var("HOME")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env::var("USERPROFILE")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
        .unwrap_or_else(|| PathBuf::from("."))
}
