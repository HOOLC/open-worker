use std::time::Duration;

use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::time::{self, MissedTickBehavior};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{info, warn};

use crate::config::RuntimeConfig;
use crate::delivery;
use crate::state::AppState;
use zork_slack::{parse_socket_payload, BotIdentity};

pub async fn run_socket(state: AppState, mut shutdown: tokio::sync::watch::Receiver<bool>) {
    let mut lease_config = state.config.clone();
    loop {
        if tokio::select! {
            _ = shutdown.changed() => true,
            _ = std::future::ready(false) => false,
        } {
            info!("slack socket loop stopping");
            return;
        }
        reload_slack(&mut lease_config);
        if !has_slack(&lease_config) {
            info!("waiting for Slack tokens in config.json");
            time::sleep(Duration::from_secs(1)).await;
            continue;
        }
        match connect_once(&state, &lease_config).await {
            Ok(()) => info!("slack socket ended"),
            Err(error) => warn!(error = %format!("{error:#}"), "slack socket failed"),
        }
        time::sleep(Duration::from_secs(1)).await;
    }
}

fn reload_slack(config: &mut RuntimeConfig) {
    if let Ok(file) = zork_config::load_config(&config.data_root) {
        config.slack_app_token = file.slack.app_token.trim().to_string();
        config.slack_bot_token = file.slack.bot_token.trim().to_string();
        config.slack_api_base_url = zork_config::slack_api_base_url(&file);
    }
}

fn has_slack(config: &RuntimeConfig) -> bool {
    !config.slack_app_token.is_empty() && !config.slack_bot_token.is_empty()
}

async fn connect_once(state: &AppState, config: &RuntimeConfig) -> Result<()> {
    let bot = fetch_bot_identity(config).await?;
    info!(user_id = %bot.user_id, "resolved slack bot identity");
    let url = open_connection(config).await?;
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
                        handle_envelope(state, &envelope, &bot).await?;
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

#[derive(Debug, Deserialize)]
struct SlackEnvelope {
    #[serde(default)]
    envelope_id: Option<String>,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    payload: Option<Value>,
}

async fn handle_envelope(
    state: &AppState,
    envelope: &SlackEnvelope,
    bot: &BotIdentity,
) -> Result<()> {
    if envelope.kind == "hello" || envelope.kind == "disconnect" {
        return Ok(());
    }
    let Some(payload) = envelope.payload.as_ref() else {
        return Ok(());
    };
    let Some((_event_id, inbound)) = parse_socket_payload(&envelope.kind, payload, bot) else {
        return Ok(());
    };
    let Some(event) = crate::inbound::parse_inbound_value(&inbound) else {
        return Ok(());
    };
    delivery::handle_event_with_bot(state, &event).await
}

#[derive(Debug, Deserialize)]
struct SlackApiResponse {
    ok: bool,
    error: Option<String>,
    url: Option<String>,
    user_id: Option<String>,
    user: Option<String>,
    bot_id: Option<String>,
    app_id: Option<String>,
}

async fn open_connection(config: &RuntimeConfig) -> Result<String> {
    let http = reqwest::Client::builder().no_proxy().build()?;
    let url = format!("{}/apps.connections.open", config.slack_api_base_url);
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

async fn fetch_bot_identity(config: &RuntimeConfig) -> Result<BotIdentity> {
    let http = reqwest::Client::builder().no_proxy().build()?;
    let url = format!("{}/auth.test", config.slack_api_base_url);
    let response = http
        .post(&url)
        .header(
            "authorization",
            format!("Bearer {}", config.slack_bot_token),
        )
        .header(
            "content-type",
            "application/x-www-form-urlencoded; charset=utf-8",
        )
        .send()
        .await
        .context("auth.test")?;
    let payload: SlackApiResponse = response.json().await.context("auth.test json")?;
    if !payload.ok {
        anyhow::bail!(
            "Slack API error for auth.test: {}",
            payload.error.unwrap_or_else(|| "unknown_error".into())
        );
    }
    let user_id = payload
        .user_id
        .filter(|value| !value.trim().is_empty())
        .context("auth.test missing user_id")?;
    Ok(BotIdentity {
        user_id,
        bot_id: payload.bot_id.filter(|value| !value.trim().is_empty()),
        app_id: payload.app_id.filter(|value| !value.trim().is_empty()),
        username: payload.user.filter(|value| !value.trim().is_empty()),
        display_name: None,
        real_name: None,
        surface: "Slack".into(),
    })
}
