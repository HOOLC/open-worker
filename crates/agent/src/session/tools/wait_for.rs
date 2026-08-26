use serde_json::json;

use crate::session::runtime::{ToolDefinition, WAIT_FOR_TOOL_NAME};
use crate::session::state::{WAIT_MAX_SECONDS, WAIT_MIN_SECONDS};

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: WAIT_FOR_TOOL_NAME.to_owned(),
        description: "Pause this session until new input or a runtime notification arrives."
            .to_owned(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "reason": {"type": "string"},
                "timeout_seconds": {
                    "type": "integer",
                    "minimum": WAIT_MIN_SECONDS,
                    "maximum": WAIT_MAX_SECONDS
                }
            },
            "required": ["reason"],
            "additionalProperties": false
        }),
    }
}
