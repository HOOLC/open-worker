use anyhow::{Context, Result};
use reqwest::StatusCode;
use serde_json::{json, Value};
use tracing::info;

use crate::config::RuntimeConfig;
use crate::db::{GatewayDb, SessionRow};
use crate::inbound::InboundEvent;
use crate::jobs::JobEvent;
use crate::slack::BotSelf;
use crate::state::AppState;

pub async fn handle_event_with_bot(state: &AppState, event: &InboundEvent) -> Result<()> {
    if let Some(self_json) = &event.self_json {
        apply_bot(state, self_json).await;
    }
    handle_inbound(state, event.clone()).await
}

async fn apply_bot(state: &AppState, self_json: &Value) {
    let user_id = self_json
        .get("userId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if user_id.is_empty() {
        return;
    }
    let mention = self_json
        .get("mention")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("<@{user_id}>"));
    *state.bot.lock().await = Some(BotSelf {
        user_id,
        mention,
        raw: self_json.clone(),
    });
}

pub async fn handle_inbound(state: &AppState, event: InboundEvent) -> Result<()> {
    info!(
        session = %event.session_key(),
        source = %event.source,
        "chat.message.accepted"
    );
    let _ = state.db.insert_admin_event(
        "inbound",
        "session",
        Some(&event.session_key()),
        event.message_id.as_deref(),
        &json!({
            "source": event.source,
            "conversationId": event.conversation_id,
            "rootMessageId": event.root_message_id,
        }),
    );

    let creates_session = matches!(event.source.as_str(), "app_mention" | "direct_message");
    let existing = state.db.get_session(&event.session_key())?;
    if !creates_session && existing.is_none() {
        return Ok(());
    }
    let session = state.db.ensure_session(
        &event.conversation_id,
        &event.root_message_id,
        event.channel_type.as_deref(),
        (event.sender_kind == "user").then_some(event.sender_user_id.as_str()),
        event.message_id.as_deref(),
    )?;
    if let Some((name, channel_type)) = state.slack.conversation_info(&event.conversation_id).await
    {
        state
            .db
            .set_channel_metadata(&session.key, name.as_deref(), channel_type.as_deref())?;
    }
    let session = state
        .db
        .get_session(&session.key)?
        .context("session missing")?;

    if event.is_stop() {
        let stopped = match session.id.as_deref() {
            Some(session_id) => crate::agent::cancel_session(&state.config, session_id).await?,
            None => false,
        };
        state.status.clear(&session.key).await;
        state
            .slack
            .post_thread_message(
                &session.channel_id,
                &session.root_thread_ts,
                if stopped {
                    "Stopped the current run."
                } else {
                    "No active run to stop."
                },
            )
            .await
            .ok();
        state.db.touch_reply(&session.key)?;
        return Ok(());
    }
    if event.is_empty() {
        return Ok(());
    }

    let slack_message_id = event
        .message_id
        .as_deref()
        .context("Slack message is missing messageId")?;
    if state
        .db
        .inbound_status(&session.key, slack_message_id)?
        .as_deref()
        == Some("delivered")
    {
        return Ok(());
    }
    let history = if existing.is_none()
        && event.source == "app_mention"
        && event.message_id.as_deref() != Some(event.root_message_id.as_str())
    {
        load_history_text(state, &event).await
    } else {
        None
    };
    let content = format_event(state, &event, history.as_deref()).await;
    let delivery = append_to_agent(&state.config, &state.db, session.clone(), &content).await;
    state.db.record_inbound(
        &session.key,
        &session.channel_id,
        &session.root_thread_ts,
        slack_message_id,
        &event.source,
        &event.sender_user_id,
        &event.text,
        event.channel_type.as_deref(),
        if delivery.is_ok() {
            "delivered"
        } else {
            "blocked"
        },
    )?;
    delivery
}

pub async fn handle_job_event(
    config: &RuntimeConfig,
    db: &GatewayDb,
    event: JobEvent,
) -> Result<()> {
    let key = format!("{}:{}", event.conversation_id, event.root_message_id);
    let session = db.get_session(&key)?.context("session_not_found")?;
    let content = format!(
        "A broker-managed background job reported a new asynchronous event for this session.\njob_id: {}\njob_kind: {}\nevent_kind: {}\nsummary: {}",
        event.job_id, event.kind, event.event_kind, event.summary
    );
    append_to_agent(config, db, session, &content).await
}

async fn append_to_agent(
    config: &RuntimeConfig,
    db: &GatewayDb,
    session: SessionRow,
    content: &str,
) -> Result<()> {
    let agent_id = crate::agent::ensure_session(config, db, &session).await?;
    crate::agent::append_mailbox(config, &agent_id, content).await?;
    Ok(())
}

pub async fn reset_session(state: &AppState, session_key: &str) -> Result<bool> {
    let session = state
        .db
        .get_session(session_key)?
        .with_context(|| format!("Unknown session: {session_key}"))?;
    if let Some(session_id) = session.id.as_deref() {
        delete_agent_session(state, session_id).await?;
    }
    state.db.clear_agent_session(session_key)?;
    state.status.clear(session_key).await;

    let current = state
        .db
        .get_session(session_key)?
        .with_context(|| format!("Unknown session: {session_key}"))?;
    let history =
        load_thread_history_text(state, &current.channel_id, &current.root_thread_ts, None).await;
    let instruction = "A session reset was requested by an administrator. Treat earlier Agent history as cleared, rebuild context from the current Slack thread, and continue only work that is still relevant.";
    let content = match history {
        Some(history) => format!("{history}\n\n{instruction}"),
        None => instruction.to_owned(),
    };
    append_to_agent(&state.config, &state.db, current, &content).await?;
    Ok(true)
}

pub async fn delete_session(state: &AppState, session_key: &str) -> Result<bool> {
    let session = state
        .db
        .get_session(session_key)?
        .with_context(|| format!("Unknown session: {session_key}"))?;
    if let Some(session_id) = session.id.as_deref() {
        delete_agent_session(state, session_id).await?;
    }
    state.db.delete_session(session_key)
}

async fn delete_agent_session(state: &AppState, agent_id: &str) -> Result<()> {
    let request = reqwest::Client::builder()
        .no_proxy()
        .build()?
        .delete(format!(
            "{}/v1/sessions/{agent_id}",
            crate::agent::base_url(&state.config)
        ));
    let response = crate::agent::authenticate(&state.config, request)
        .send()
        .await
        .context("delete canonical Agent session")?;
    let status = response.status();
    if status != StatusCode::NO_CONTENT && status != StatusCode::NOT_FOUND {
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Agent session deletion failed ({status}): {body}");
    }
    Ok(())
}

async fn format_event(
    state: &AppState,
    event: &InboundEvent,
    earlier_thread_context: Option<&str>,
) -> String {
    let sender = if event.sender_kind == "user" {
        state.slack.user_identity(&event.sender_user_id).await
    } else {
        None
    };
    let payload = json!({
        "source": event.source,
        "message_ts": event.message_id,
        "sender": {
            "user_id": event.sender_user_id,
            "kind": event.sender_kind,
            "display_name": sender.as_ref().and_then(|value| value.get("displayName").cloned()),
        },
        "mentioned_user_ids": event.mentioned_user_ids,
        "text": if event.text.trim().is_empty() { "[no text body]".into() } else { event.text.clone() },
        "attachments": event.attachments,
    });
    let current = format!(
        "A new message arrived in the Slack thread. Carefully judge whether it requires a reply or action from you.\nstructured_message_json:\n```json\n{}\n```",
        serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string())
    );
    match earlier_thread_context.filter(|value| !value.trim().is_empty()) {
        Some(context) => {
            format!("{context}\n\nCurrent Slack message requiring attention:\n{current}")
        }
        None => current,
    }
}

async fn load_history_text(state: &AppState, event: &InboundEvent) -> Option<String> {
    load_thread_history_text(
        state,
        &event.conversation_id,
        &event.root_message_id,
        event.message_id.as_deref(),
    )
    .await
}

async fn load_thread_history_text(
    state: &AppState,
    conversation_id: &str,
    root_message_id: &str,
    before_message_id: Option<&str>,
) -> Option<String> {
    let history = state
        .slack
        .thread_history(
            conversation_id,
            root_message_id,
            before_message_id,
            Some(state.config.slack_initial_thread_history_count),
        )
        .await
        .ok()?;
    let messages = history.get("messages").and_then(Value::as_array)?;
    if messages.is_empty() {
        return None;
    }
    Some(format!(
        "Earlier Slack thread context before the current message. Treat these history items as context only; do not reply to them individually.\nhistory_count: {}\n{}",
        messages.len(),
        serde_json::to_string_pretty(messages).unwrap_or_default()
    ))
}

pub async fn post_message(
    state: &AppState,
    conversation_id: &str,
    root_message_id: &str,
    text: &str,
    _kind: Option<&str>,
    _reason: Option<&str>,
) -> Result<()> {
    let key = format!("{conversation_id}:{root_message_id}");
    state
        .slack
        .post_thread_message(conversation_id, root_message_id, text)
        .await?;
    if state.db.get_session(&key)?.is_some() {
        state.db.touch_reply(&key)?;
    }
    Ok(())
}
