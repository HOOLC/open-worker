use serde_json::{json, Value};

use crate::session::runtime::{ToolDefinition, ToolError, CONTEXT_HANDOFF_TOOL_NAME};

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: CONTEXT_HANDOFF_TOOL_NAME.to_owned(),
        description: "Submit the complete context handoff document when the runtime explicitly requests a context handoff. Do not call this tool during ordinary work. It must be the only tool call in that model response."
            .to_owned(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "document": {
                    "type": "string",
                    "minLength": 1,
                    "description": "The complete handoff document: authoritative user requirements, long-term goal, current goal, current state, conclusions, evidence locations, and next actions."
                }
            },
            "required": ["document"],
            "additionalProperties": false
        }),
    }
}

pub fn document(input: &Value) -> Result<String, ToolError> {
    let object = input.as_object().ok_or(ToolError::InvalidInvocation)?;
    if object.len() != 1 {
        return Err(ToolError::InvalidInvocation);
    }
    let document = object
        .get("document")
        .and_then(Value::as_str)
        .ok_or(ToolError::InvalidInvocation)?;
    if document.trim().is_empty() {
        return Err(ToolError::InvalidInvocation);
    }
    Ok(document.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_is_validated_but_not_rewritten() {
        assert_eq!(
            document(&json!({"document": "  durable handoff\n"})).unwrap(),
            "  durable handoff\n"
        );
        assert!(document(&json!({"document": "   \n"})).is_err());
        assert!(document(&json!({"document": "handoff", "extra": true})).is_err());
    }
}
