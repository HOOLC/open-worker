//! Authenticated human responses. Agent tools can publish requests, but cannot
//! use their tool session identity to impersonate this confirmation endpoint.
use super::*;
use crate::db::agents::{AgentRole, NodeAgent};
use anyhow::{ensure, Context};
use zork_client_types::interaction::{Content, MessageContent, Request, Response as Input};

pub(super) async fn respond(
    State(state): State<NodeState>,
    headers: HeaderMap,
    Path((chat_id, message_id)): Path<(String, String)>,
    Json(input): Json<Input>,
) -> Response {
    if !authorized(&state, &headers) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Node administrator token required",
        );
    }
    let result: anyhow::Result<Value> = async {
        let chat = state.app.db.chat(&chat_id)?.channel.chat_id;
        let _guard = state
            .app
            .entries
            .lock_local_task(&format!("interaction:{chat}:{message_id}"))
            .await;
        if let Some(result) = state.app.db.interaction_result(&chat, &message_id)? {
            return Ok(state
                .app
                .entries
                .message_json(&state.app.db.chat_visible_message(&result.message_id)?));
        }
        let message = state.app.db.chat_message(&chat, &message_id)?;
        let Some(MessageContent {
            content: Content::Request { request },
            ..
        }) = message.interaction.as_ref().and_then(MessageContent::parse)
        else {
            anyhow::bail!("interaction_request_required");
        };
        request.validate().map_err(anyhow::Error::msg)?;
        let mut prepared = None;
        if input.accept {
            let values = request
                .validate_values(&input.values)
                .map_err(|_| anyhow::anyhow!("invalid_interaction_input"))?;
            let configuration = match &request {
                Request::CreateAgent { config } => Some((None, config)),
                Request::UpdateAgent {
                    agent_id, config, ..
                } => Some((Some(agent_id), config)),
                Request::Input { .. } => None,
            };
            if let Some((existing, config)) = configuration {
                let mut agent = if let Some(id) = existing {
                    state.app.db.node_agent(id)?.context("agent_not_found")?
                } else {
                    NodeAgent {
                        id: ulid::Ulid::new().to_string(),
                        name: String::new(),
                        avatar: None,
                        role: AgentRole::Worker,
                        profile_id: String::new(),
                        model: String::new(),
                        thinking: String::new(),
                        instructions: String::new(),
                        skill_paths: vec![],
                        allowed_leaders: vec![],
                        session_key: None,
                        session_id: None,
                    }
                };
                let mut fields = serde_json::to_value(config)?;
                fields["name"] = json!(values["name"]);
                fields["instructions"] = json!(values["instructions"]);
                crate::channels::apply_configuration(&state.app, &mut agent, &fields).await?;
                prepared = Some(agent);
            }
        }
        // The authority and immutable proposal are rechecked at the commit.
        ensure!(authorized(&state, &headers), "node_management_denied");
        let message = state.app.db.respond_to_interaction(
            &chat,
            &message_id,
            &input,
            prepared.as_ref(),
            "local-user",
        )?;
        let row = state.app.db.chat_visible_message(&message.message_id)?;
        state.app.entries.publish_visible_message(&row);
        Ok(state.app.entries.message_json(&row))
    }
    .await;
    match result {
        Ok(message) => Json(json!({"message":message})).into_response(),
        Err(err) => error(StatusCode::BAD_REQUEST, &err.to_string()),
    }
}
