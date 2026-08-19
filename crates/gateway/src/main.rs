mod config;
mod db;
mod http;
mod slack;

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tokio::time;
use tracing::{error, info};

use crate::config::GatewayConfig;
use crate::db::GatewayDb;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env().add_directive("info".parse()?),
        )
        .init();

    let config = GatewayConfig::from_env()?;
    let db = Arc::new(GatewayDb::open(&config.state_dir)?);
    if !db.try_acquire_role("gateway", &config.process_id, config.lease_ttl_ms)? {
        anyhow::bail!("another gateway holds the process lease");
    }
    info!(process_id = %config.process_id, state = %config.state_dir.display(), "gateway starting");

    let http_client = reqwest::Client::builder().no_proxy().build()?;
    let slack_config = config.clone();
    let slack_db = db.clone();
    let slack_http = http_client.clone();
    let lease_config = config.clone();
    let lease_db = db.clone();
    let http_config = config.clone();
    let http_for_server = http_client.clone();

    tokio::select! {
        result = slack::run_socket(slack_config, slack_db, slack_http) => {
            if let Err(error) = result {
                error!(error = %error, "slack loop exited");
            }
        }
        result = http::serve(http_config, http_for_server) => {
            if let Err(error) = result {
                error!(error = %error, "http loop exited");
            }
        }
        _ = renew_gateway_lease(lease_config, lease_db) => {}
        _ = shutdown_signal() => {
            info!("gateway shutting down");
        }
    }
    if let Err(error) = db.release_role("gateway", &config.process_id) {
        error!(error = %error, "release gateway lease");
    }
    Ok(())
}

async fn renew_gateway_lease(config: GatewayConfig, db: Arc<GatewayDb>) {
    let mut ticker = time::interval(Duration::from_millis(
        (config.lease_ttl_ms / 3).max(1) as u64
    ));
    loop {
        ticker.tick().await;
        match db.try_acquire_role("gateway", &config.process_id, config.lease_ttl_ms) {
            Ok(true) => {}
            Ok(false) => {
                error!("lost gateway lease");
                return;
            }
            Err(error) => error!(error = %error, "renew gateway lease"),
        }
    }
}

async fn shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("listen for SIGTERM");
        tokio::select! {
            _ = ctrl_c => {}
            _ = sigterm.recv() => {}
        }
        return;
    }
    #[cfg(not(unix))]
    {
        let _ = ctrl_c.await;
    }
}
