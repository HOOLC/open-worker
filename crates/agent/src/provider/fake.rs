use serde_json::Value;
use zork_agent::session::runtime::{
    ModelError, ModelExecutor, ModelOutcome, ModelRequest, ProfileExecution, END_TOOL_NAME,
};
use zork_agent::session::state::{ToolCall, TranscriptRole};

pub struct FakeProvider;

impl ModelExecutor for FakeProvider {
    fn complete<'a>(
        &'a self,
        request: &'a ModelRequest,
        _execution: ProfileExecution,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<ModelOutcome, ModelError>> + Send + 'a>,
    > {
        Box::pin(async move {
            let latest = request.transcript.last();
            if latest.is_some_and(|message| message.role == TranscriptRole::Assistant)
                && request
                    .tools
                    .iter()
                    .any(|definition| definition.name == END_TOOL_NAME)
            {
                return Ok(ModelOutcome {
                    text: String::new(),
                    tool_calls: vec![ToolCall {
                        tool_call_id: format!("fake-end:{}", request.round_id),
                        tool_name: END_TOOL_NAME.to_owned(),
                        arguments: serde_json::json!({}),
                    }],
                    provider_context: None,
                    usage: None,
                    provider_input: None,
                });
            }
            if let Some(message) = latest.filter(|message| message.role == TranscriptRole::User) {
                if let Ok(Value::Object(document)) = serde_json::from_str(&message.content) {
                    if let Some(Value::Array(tools)) = document.get("fake_tools") {
                        let calls = tools
                            .iter()
                            .enumerate()
                            .filter_map(|(index, tool)| {
                                let Value::Object(tool) = tool else {
                                    return None;
                                };
                                let name = tool.get("name")?.as_str()?;
                                let input = tool.get("input")?.clone();
                                request
                                    .tools
                                    .iter()
                                    .any(|definition| definition.name == name)
                                    .then_some((index, name, input))
                            })
                            .map(|(index, name, input)| {
                                Ok(ToolCall {
                                    tool_call_id: format!("fake-tool:{}:{index}", request.round_id),
                                    tool_name: name.to_owned(),
                                    arguments: input,
                                })
                            })
                            .collect::<Result<Vec<_>, ModelError>>()?;
                        if !calls.is_empty() {
                            return Ok(ModelOutcome {
                                text: String::new(),
                                tool_calls: calls,
                                provider_context: None,
                                usage: None,
                                provider_input: None,
                            });
                        }
                    }
                    if let Some(Value::Object(tool)) = document.get("fake_tool") {
                        let name = tool.get("name").and_then(Value::as_str);
                        let input = tool.get("input").cloned();
                        if let (Some(name), Some(input)) = (name, input) {
                            if request
                                .tools
                                .iter()
                                .any(|definition| definition.name == name)
                                && input.is_object()
                            {
                                return Ok(ModelOutcome {
                                    text: String::new(),
                                    tool_calls: vec![ToolCall {
                                        tool_call_id: format!("fake-tool:{}", request.round_id),
                                        tool_name: name.to_owned(),
                                        arguments: input,
                                    }],
                                    provider_context: None,
                                    usage: None,
                                    provider_input: None,
                                });
                            }
                        }
                    }
                }
            }
            let text = latest
                .filter(|message| {
                    matches!(message.role, TranscriptRole::User | TranscriptRole::Tool)
                })
                .map(|message| message.content.to_string())
                .unwrap_or_default();
            request.stream_observer.text_delta(
                &request.session_id,
                &request.activation_id,
                &request.round_id,
                &text,
            );
            Ok(ModelOutcome {
                text,
                tool_calls: Vec::new(),
                provider_context: None,
                usage: None,
                provider_input: None,
            })
        })
    }
}
