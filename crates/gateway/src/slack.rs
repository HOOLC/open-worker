use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::time::{self, MissedTickBehavior};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{info, warn};

use crate::config::GatewayConfig;
use crate::db::GatewayDb;

#[derive(Debug, Deserialize)]
struct SlackApiResponse {
    ok: bool,
    error: Option<String>,
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SlackEnvelope {
    #[serde(default)]
    envelope_id: Option<String>,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    payload: Option<Value>,
}

pub async fn run_socket(
    config: GatewayConfig,
    db: Arc<GatewayDb>,
    http: reqwest::Client,
) -> Result<()> {
    loop {
        match connect_once(&config, &db, &http).await {
            Ok(()) => info!("slack socket ended"),
            Err(error) => warn!(error = %error, "slack socket failed"),
        }
        time::sleep(Duration::from_secs(1)).await;
    }
}

async fn connect_once(
    config: &GatewayConfig,
    db: &GatewayDb,
    http: &reqwest::Client,
) -> Result<()> {
    let url = open_connection(config, http).await?;
    info!(url, "connecting slack socket");
    let (stream, _) = connect_async(&url)
        .await
        .context("slack websocket connect")?;
    let (mut write, mut read) = stream.split();
    info!("connected to Slack Socket Mode");

    let mut heartbeat = time::interval(Duration::from_secs(30));
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut awaiting_pong = false;

    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                if awaiting_pong {
                    anyhow::bail!("slack websocket heartbeat timed out");
                }
                awaiting_pong = true;
                write.send(Message::Ping(Vec::new().into())).await.context("slack ping")?;
            }
            message = read.next() => {
                let Some(message) = message else {
                    anyhow::bail!("slack websocket closed");
                };
                match message.context("slack websocket read")? {
                    Message::Pong(_) => awaiting_pong = false,
                    Message::Ping(payload) => {
                        write.send(Message::Pong(payload)).await.context("slack pong")?;
                    }
                    Message::Close(_) => anyhow::bail!("slack websocket closed"),
                    Message::Text(text) => {
                        let envelope: SlackEnvelope = serde_json::from_str(&text).context("slack envelope json")?;
                        handle_envelope(db, &envelope).await?;
                        if envelope.kind == "disconnect" {
                            anyhow::bail!("slack requested disconnect");
                        }
                        if let Some(envelope_id) = envelope.envelope_id.as_deref() {
                            write.send(Message::Text(json!({ "envelope_id": envelope_id }).to_string().into())).await.context("slack ack")?;
                        }
                    }
                    Message::Binary(_) | Message::Frame(_) => {}
                }
            }
        }
    }
}

async fn open_connection(config: &GatewayConfig, http: &reqwest::Client) -> Result<String> {
    let url = format!(
        "{}/{}",
        config.slack_api_base_url,
        config.slack_socket_open_path.trim_start_matches('/')
    );
    let response = http
        .post(&url)
        .header(
            "authorization",
            format!("Bearer {}", config.slack_app_token),
        )
        .header(
            "content-type",
            "application/x-www-form-urlencoded; charset=utf-8",
        )
        .send()
        .await
        .context("apps.connections.open")?;
    let payload: SlackApiResponse = response
        .json()
        .await
        .context("apps.connections.open json")?;
    if !payload.ok {
        anyhow::bail!(
            "Slack API error for apps.connections.open: {}",
            payload.error.unwrap_or_else(|| "unknown_error".into())
        );
    }
    payload.url.context("apps.connections.open missing url")
}

async fn handle_envelope(db: &GatewayDb, envelope: &SlackEnvelope) -> Result<()> {
    if envelope.kind == "hello" || envelope.kind == "disconnect" {
        return Ok(());
    }
    if envelope.kind != "events_api" && envelope.kind != "interactive" {
        return Ok(());
    }
    let Some(payload) = envelope.payload.as_ref() else {
        return Ok(());
    };
    let id = payload
        .get("event_id")
        .and_then(Value::as_str)
        .or(envelope.envelope_id.as_deref())
        .unwrap_or("");
    if id.is_empty() {
        return Ok(());
    }
    db.enqueue_inbound(id, "slack", payload)?;
    Ok(())
}
