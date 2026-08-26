use serde_json::json;

use crate::session::runtime::{ToolDefinition, END_TOOL_NAME};

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: END_TOOL_NAME.to_owned(),
        description: "The only way to finish the current activation successfully. You must call end after all work and other tool calls have completed; an assistant response does not finish the activation. end must be the only tool call in this model response."
            .to_owned(),
        input_schema: json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        }),
    }
}
