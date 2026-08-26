use serde_json::{json, Value};

use crate::session::runtime::{ToolDefinition, ToolError, READ_CONTEXT_HANDOFF_TOOL_NAME};
use crate::session::state::ContextHandoffDocument;

const CONTENT_CHUNK_BYTES: usize = 8 * 1024;

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: READ_CONTEXT_HANDOFF_TOOL_NAME.to_owned(),
        description: "Reread a bounded chunk of the latest durable context handoff document for this session. An optional handoff_id asserts the expected latest document; continue with next_content_offset until it is null.".to_owned(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "handoff_id": {"type": "string", "minLength": 1},
                "content_offset": {"type": "integer", "minimum": 0}
            },
            "additionalProperties": false
        }),
    }
}

pub fn execute(handoff: &ContextHandoffDocument, input: &Value) -> Result<Value, ToolError> {
    let requested = input.get("handoff_id").and_then(Value::as_str);
    if requested.is_some_and(|requested| requested != handoff.handoff_id) {
        return Err(ToolError::InvalidInvocation);
    }
    let offset = input
        .get("content_offset")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .try_into()
        .map_err(|_| ToolError::InvalidInvocation)?;
    if offset > handoff.document.len() || !handoff.document.is_char_boundary(offset) {
        return Err(ToolError::InvalidInvocation);
    }
    let mut end = offset
        .saturating_add(CONTENT_CHUNK_BYTES)
        .min(handoff.document.len());
    while end > offset && !handoff.document.is_char_boundary(end) {
        end -= 1;
    }
    Ok(json!({
        "schema": "zork.context-handoff-read.v1",
        "handoff_id": handoff.handoff_id,
        "previous_handoff_id": handoff.previous_handoff_id,
        "generation": handoff.next_generation,
        "covered_through_message_id": handoff.covered_through_message_id,
        "document": {
            "text": &handoff.document[offset..end],
            "content_offset": offset,
            "next_content_offset": (end < handoff.document.len()).then_some(end),
            "content_bytes": handoff.document.len(),
        },
    }))
}
