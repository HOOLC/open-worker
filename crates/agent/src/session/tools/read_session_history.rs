use serde_json::{json, Value};

use crate::session::runtime::{ToolDefinition, ToolError, READ_SESSION_HISTORY_TOOL_NAME};
use crate::session::state::{EventRecord, SessionEvent, TranscriptMessage, TranscriptRole};

const CONTENT_CHUNK_BYTES: usize = 16 * 1024;
const PREVIEW_BYTES: usize = 1_024;
const DEFAULT_MESSAGE_LIMIT: usize = 8;
const MAX_MESSAGE_LIMIT: usize = 16;

pub(super) enum HistoryRequest {
    List {
        before_event_id: Option<String>,
        limit: usize,
    },
    Content {
        event_id: String,
        content_offset: usize,
    },
}

pub(super) struct HistoryMessage {
    pub event_id: String,
    pub message: TranscriptMessage,
}

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: READ_SESSION_HISTORY_TOOL_NAME.to_owned(),
        description: "Read this session's durable message history by immutable event ULID. Without event_id, list a bounded chronological page ending strictly before before_event_id. With event_id, read a bounded content chunk starting at content_offset.".to_owned(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "before_event_id": {"type": "string", "minLength": 1},
                "limit": {"type": "integer", "minimum": 1, "maximum": MAX_MESSAGE_LIMIT},
                "event_id": {"type": "string", "minLength": 1},
                "content_offset": {"type": "integer", "minimum": 0}
            },
            "additionalProperties": false
        }),
    }
}

pub(super) fn parse(input: &Value) -> Result<HistoryRequest, ToolError> {
    let event_id = input.get("event_id").and_then(Value::as_str);
    let before_event_id = input.get("before_event_id").and_then(Value::as_str);
    if let Some(event_id) = event_id {
        if event_id.is_empty() || before_event_id.is_some() || input.get("limit").is_some() {
            return Err(ToolError::InvalidInvocation);
        }
        let content_offset = input
            .get("content_offset")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            .try_into()
            .map_err(|_| ToolError::InvalidInvocation)?;
        return Ok(HistoryRequest::Content {
            event_id: event_id.to_owned(),
            content_offset,
        });
    }
    if input.get("content_offset").is_some() || before_event_id.is_some_and(str::is_empty) {
        return Err(ToolError::InvalidInvocation);
    }
    let limit = input
        .get("limit")
        .and_then(Value::as_u64)
        .map(usize::try_from)
        .transpose()
        .map_err(|_| ToolError::InvalidInvocation)?
        .unwrap_or(DEFAULT_MESSAGE_LIMIT);
    if !(1..=MAX_MESSAGE_LIMIT).contains(&limit) {
        return Err(ToolError::InvalidInvocation);
    }
    Ok(HistoryRequest::List {
        before_event_id: before_event_id.map(str::to_owned),
        limit,
    })
}

pub(super) fn message_from_record(record: &EventRecord) -> Option<HistoryMessage> {
    let message = match &record.event {
        SessionEvent::MailboxMessageAppended { message } => TranscriptMessage {
            message_id: message.message_id.clone(),
            role: TranscriptRole::User,
            content: message.content.clone(),
            is_error: false,
            tool_call_id: None,
            tool_calls: Vec::new(),
            provider_context: None,
            source_mailbox_seq: Some(message.mailbox_seq),
        },
        SessionEvent::MessageAppended { message, .. } => message.clone(),
        _ => return None,
    };
    Some(HistoryMessage {
        event_id: record.event_id.clone(),
        message,
    })
}

pub(super) fn content(history: &HistoryMessage, offset: usize) -> Result<Value, ToolError> {
    let message = &history.message;
    if offset > message.content.len() || !message.content.is_char_boundary(offset) {
        return Err(ToolError::InvalidInvocation);
    }
    let mut end = offset
        .saturating_add(CONTENT_CHUNK_BYTES)
        .min(message.content.len());
    while end > offset && !message.content.is_char_boundary(end) {
        end -= 1;
    }
    Ok(json!({
        "schema": "zork.session-history-read.v1",
        "mode": "content",
        "event_id": history.event_id,
        "role": message.role,
        "tool_call_id": message.tool_call_id,
        "tool_calls": message.tool_calls,
        "content_offset": offset,
        "content": &message.content[offset..end],
        "next_content_offset": (end < message.content.len()).then_some(end),
        "content_bytes": message.content.len(),
    }))
}

pub(super) fn list(messages: &[HistoryMessage], has_older: bool) -> Value {
    let items = messages
        .iter()
        .map(|history| {
            let message = &history.message;
            let preview = bounded_utf8_prefix(&message.content, PREVIEW_BYTES);
            json!({
                "event_id": history.event_id,
                "role": message.role,
                "content_preview": preview,
                "content_bytes": message.content.len(),
                "content_truncated": preview.len() < message.content.len(),
                "tool_call_id": message.tool_call_id,
                "tool_calls": message.tool_calls,
            })
        })
        .collect::<Vec<_>>();
    json!({
        "schema": "zork.session-history-read.v1",
        "mode": "list",
        "messages": items,
        "next_before_event_id": if has_older {
            messages.first().map(|message| message.event_id.clone())
        } else {
            None
        },
    })
}

fn bounded_utf8_prefix(value: &str, maximum_bytes: usize) -> &str {
    if value.len() <= maximum_bytes {
        return value;
    }
    let mut end = maximum_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn history_contract_uses_event_ulids_for_paging_and_point_reads() {
        let definition = definition();
        let properties = definition.input_schema["properties"].as_object().unwrap();
        assert!(properties.contains_key("before_event_id"));
        assert!(properties.contains_key("event_id"));
        assert!(!properties.contains_key("before_message_id"));
        assert!(!properties.contains_key("message_id"));
    }

    #[test]
    fn history_results_expose_only_the_event_ulid() {
        let history = HistoryMessage {
            event_id: "01K00000000000000000000000".to_owned(),
            message: TranscriptMessage {
                message_id: "01K00000000000000000000000".to_owned(),
                role: TranscriptRole::Assistant,
                content: Arc::from("result"),
                is_error: false,
                tool_call_id: None,
                tool_calls: Vec::new(),
                provider_context: None,
                source_mailbox_seq: None,
            },
        };

        assert!(list(std::slice::from_ref(&history), false)["messages"][0]
            .get("message_id")
            .is_none());
        assert!(content(&history, 0).unwrap().get("message_id").is_none());
    }
}
