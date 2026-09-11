//! Requests and final outcomes are immutable messages. The unique root reference
//! is also the operation's durable admission record, across clients and restarts.
use super::*;
use crate::db::agents::{AgentRole, NodeAgent};
use zork_client_types::interaction::{
    Content, MessageContent, Outcome, Request, Resolution, Response,
};
#[cfg(test)]
mod tests;

pub(super) fn initialize(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS chat_interaction_messages(
        message_id TEXT PRIMARY KEY REFERENCES visible_messages(message_id),
        request_message_id TEXT UNIQUE REFERENCES visible_messages(message_id),
        value TEXT NOT NULL);",
    )?;
    Ok(())
}

pub(super) fn content(conn: &Connection, id: &str) -> Result<Option<Value>> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM chat_interaction_messages WHERE message_id=?1",
            [id],
            |r| r.get(0),
        )
        .optional()?;
    raw.map(|v| serde_json::from_str(&v).map_err(Into::into))
        .transpose()
}

pub(super) fn insert(conn: &Connection, id: &str, content: &MessageContent) -> Result<()> {
    let root = match &content.content {
        Content::Request { request } => {
            request.validate().map_err(anyhow::Error::msg)?;
            None
        }
        Content::Result { result } => Some(result.request_message_id.as_str()),
    };
    conn.execute("INSERT INTO chat_interaction_messages(message_id,request_message_id,value) VALUES(?1,?2,?3)", params![id, root, serde_json::to_string(content)?])?;
    Ok(())
}

fn resolution(conn: &Connection, root: &str) -> Result<Option<Message>> {
    let id: Option<String> = conn
        .query_row(
            "SELECT message_id FROM chat_interaction_messages WHERE request_message_id=?1",
            [root],
            |r| r.get(0),
        )
        .optional()?;
    id.map(|id| message(conn, &id)).transpose()
}

impl GatewayDb {
    pub fn interaction_result(&self, chat: &str, root: &str) -> Result<Option<Message>> {
        let conn = self.conn.lock().expect("db mutex");
        let request = message(&conn, root)?;
        anyhow::ensure!(request.chat_id == chat, "interaction_chat_mismatch");
        resolution(&conn, root)
    }

    /// Commit the effect, immutable result, participant notification and response
    /// receipt together. Preparing model/skill choices happens before this call.
    pub fn respond_to_interaction(
        &self,
        chat: &str,
        root: &str,
        response: &Response,
        prepared_agent: Option<&NodeAgent>,
        actor: &str,
    ) -> Result<Message> {
        let mut conn = self.conn.lock().expect("db mutex");
        let tx = conn.transaction()?;
        let source = message(&tx, root)?;
        anyhow::ensure!(source.chat_id == chat, "interaction_chat_mismatch");
        if let Some(result) = resolution(&tx, root)? {
            return Ok(result);
        }
        let Some(MessageContent {
            content: Content::Request { request },
            ..
        }) = source.interaction.as_ref().and_then(MessageContent::parse)
        else {
            anyhow::bail!("interaction_request_required");
        };
        request.validate().map_err(anyhow::Error::msg)?;
        anyhow::ensure!(
            !response.response_id.is_empty()
                && response.response_id.len() <= 120
                && response
                    .response_id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-'),
            "invalid_response_id"
        );
        let values = if response.accept {
            request
                .validate_values(&response.values)
                .map_err(|_| anyhow::anyhow!("invalid_interaction_input"))?
        } else {
            anyhow::ensure!(response.values.is_empty(), "unexpected_decline_input");
            Default::default()
        };
        let output = if !response.accept {
            Value::Null
        } else {
            match &request {
                Request::CreateAgent { .. } => {
                    let agent = prepared_agent.context("agent_configuration_required")?;
                    anyhow::ensure!(
                        agent.role == AgentRole::Worker
                            && agent.name == values["name"].trim()
                            && agent.instructions == values["instructions"],
                        "agent_configuration_mismatch"
                    );
                    agents::save_agent(&tx, agent, None)?
                }
                Request::UpdateAgent {
                    agent_id,
                    expected_revision,
                    ..
                } => {
                    let agent = prepared_agent.context("agent_configuration_required")?;
                    anyhow::ensure!(
                        agent.id == *agent_id
                            && agent.name == values["name"].trim()
                            && agent.instructions == values["instructions"],
                        "agent_configuration_mismatch"
                    );
                    agents::save_agent(&tx, agent, Some(expected_revision))?
                }
                Request::Input { .. } => json!({"values":values}),
            }
        };
        let text = if !response.accept {
            "Request declined.".to_owned()
        } else {
            match &request {
                Request::CreateAgent { .. } => format!(
                    "Agent created: {}",
                    output["agent"]["name"].as_str().unwrap_or_default()
                ),
                Request::UpdateAgent { .. } => format!(
                    "Agent updated: {}",
                    output["agent"]["name"].as_str().unwrap_or_default()
                ),
                Request::Input { title, .. } => {
                    format!("{title}\n{}", serde_json::to_string_pretty(&values)?)
                }
            }
        };
        let result = MessageContent::result(Resolution {
            request_message_id: root.into(),
            response_id: response.response_id.clone(),
            revision: 1,
            outcome: if response.accept {
                Outcome::Completed
            } else {
                Outcome::Declined
            },
            actor: actor.into(),
            output,
        });
        let channel = tx.query_row(
            &format!("{CHANNEL_SELECT} WHERE chat_id=?1"),
            [chat],
            map_channel,
        )?;
        let id = ulid::Ulid::new().to_string();
        let topics = messages::append_visible(
            &tx,
            &channel,
            &id,
            &Author {
                id: "zork".into(),
                kind: AuthorKind::System,
                name: Some("Zork".into()),
            },
            &text,
            Some(root),
            &[source.author.id],
            &[],
            Some(&result),
            &now_rfc3339(),
        )?;
        let message = message(&tx, &id)?;
        tx.commit()?;
        self.chat_topics.publish(topics);
        Ok(message)
    }
}
