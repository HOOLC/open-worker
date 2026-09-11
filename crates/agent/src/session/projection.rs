//! Pure projection from the current generation to provider messages.

use std::sync::Arc;

use serde_json::json;

use super::events::{ToolDelivery, ToolDeliveryMode, ToolOutcome};
use super::state::{GenerationEntry, SessionState};
use super::wire::{ProviderMessage, TranscriptRole};

pub fn provider_transcript(state: &SessionState) -> Arc<Vec<ProviderMessage>> {
    let mut messages = Vec::new();
    if let Some(prompt) = state.system_prompt.as_deref().filter(|p| !p.is_empty()) {
        messages.push(message(TranscriptRole::System, prompt, false));
    }
    messages.push(message(
        TranscriptRole::System,
        &tool_catalog_prompt(state),
        false,
    ));
    if let Some(document) = state.generation.document.as_deref() {
        append_notice(
            &mut messages,
            state,
            "context",
            &format!("Context from the previous generation:\n\n{document}"),
            false,
        );
    }

    let entries = &state.generation.entries;
    let mut consumed = vec![false; entries.len()];
    for (index, entry) in entries.iter().enumerate() {
        if consumed[index] {
            continue;
        }
        let key = index.to_string();
        match entry {
            GenerationEntry::Inputs { inputs } => {
                for input in inputs {
                    messages.push(message(TranscriptRole::User, &input.content, false));
                }
            }
            GenerationEntry::ToolChanges { changes } => {
                let lines = changes.iter().map(|change| match change {
                    super::tools::ToolChange::Added { name, .. } => format!(
                        "Tool {name} was added. If you need it, call tool.help for its current usage."
                    ),
                    super::tools::ToolChange::Updated { name, .. } => format!(
                        "Tool {name} was updated. If you need it, call tool.help for its current usage."
                    ),
                    super::tools::ToolChange::Removed { name } => format!("Tool {name} was removed."),
                }).collect::<Vec<_>>().join("\n");
                append_notice(&mut messages, state, &key, &lines, false);
            }
            GenerationEntry::Notice { message } => {
                append_notice(&mut messages, state, &key, message, false);
            }
            GenerationEntry::Outstanding { items } => {
                append_notice(
                    &mut messages,
                    state,
                    &key,
                    &format!(
                        "Unfinished items:\n{}",
                        serde_json::to_string_pretty(items).unwrap_or_else(|_| "[]".into())
                    ),
                    false,
                );
            }
            GenerationEntry::ToolDelivery { delivery } => {
                // A result not consumed with its owning assistant call is new
                // information, never a reason to rewrite an earlier request.
                append_unpaired_delivery(&mut messages, state, &key, delivery);
            }
            GenerationEntry::Assistant {
                text,
                provider_calls,
                invocations,
                provider_context,
                terminal_deliveries,
                ..
            } => {
                messages.push(ProviderMessage {
                    images: Vec::new(),
                    role: TranscriptRole::Assistant,
                    content: Arc::from(text.as_str()),
                    is_error: false,
                    runtime_generated: false,
                    tool_call_id: None,
                    tool_calls: provider_calls.clone(),
                    provider_context: provider_context.clone(),
                });
                // Only inspect this response's following input block. Match
                // by domain invocation ID, not globally by provider call ID.
                // Pending results already close the original pair; late
                // Notification deliveries are deliberately left at the tail.
                let end = entries[index + 1..]
                    .iter()
                    .position(|e| matches!(e, GenerationEntry::Assistant { .. }))
                    .map_or(entries.len(), |offset| index + 1 + offset);
                for (call_index, call) in provider_calls.iter().enumerate() {
                    let invocation = invocations
                        .get(call_index)
                        .filter(|invocation| invocation.provider_call_id == call.tool_call_id);
                    let inline = invocation.and_then(|invocation| {
                        terminal_deliveries.iter().find(|delivery| {
                            direct_invocation(delivery)
                                .is_some_and(|id| id == invocation.invocation_id)
                        })
                    });
                    let found = invocation.and_then(|invocation| {
                        (index + 1..end).find(|candidate| {
                            !consumed[*candidate] && matches!(&entries[*candidate],
                                GenerationEntry::ToolDelivery { delivery }
                                    if direct_invocation(delivery).is_some_and(|id| id == invocation.invocation_id))
                        })
                    });
                    let delivery = inline.or_else(|| {
                        found.and_then(|candidate| {
                            if let GenerationEntry::ToolDelivery { delivery } = &entries[candidate]
                            {
                                Some(delivery)
                            } else {
                                None
                            }
                        })
                    });
                    if let Some(delivery) = delivery {
                        append_direct_delivery(&mut messages, delivery);
                        if let Some(candidate) = found {
                            consumed[candidate] = true;
                        }
                    } else {
                        // Recoverable incomplete history still needs a legal
                        // pair. It must never invent a successful execution.
                        let mut pending = message(TranscriptRole::Tool,
                            "This invocation has no available result in this request. Its execution may be unfinished or interrupted; any later result will arrive as a runtime notification.", true);
                        pending.tool_call_id = Some(call.tool_call_id.clone());
                        messages.push(pending);
                    }
                }
            }
            GenerationEntry::CarriedTools { invocations } => {
                let carried = invocations.iter().map(|invocation| json!({
                    "invocation_id": invocation.invocation_id,
                    "turn_id": invocation.turn_id,
                    "tool": invocation.tool,
                    "arguments": invocation.arguments,
                    "action": invocation.activity.as_ref().map(|a| a.action.as_str()).unwrap_or_default(),
                    "started_at_ms": invocation.started_at_ms,
                })).collect::<Vec<_>>();
                append_notice(&mut messages, state, &key, &format!(
                    "These tool invocations were unfinished at the context transition. They are not recreated as provider call/result pairs. Later outcomes will arrive as notifications:\n{}",
                    serde_json::to_string_pretty(&carried).unwrap_or_else(|_| "[]".into())
                ), false);
            }
        }
    }
    Arc::new(messages)
}

fn direct_invocation(delivery: &ToolDelivery) -> Option<&str> {
    match delivery {
        ToolDelivery::Pending { invocation }
        | ToolDelivery::Result {
            invocation,
            mode: ToolDeliveryMode::Direct,
            ..
        } => Some(invocation.invocation_id.as_str()),
        ToolDelivery::Result { .. } => None,
    }
}

fn append_notice(
    messages: &mut Vec<ProviderMessage>,
    state: &SessionState,
    key: &str,
    content: &str,
    is_error: bool,
) {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(format!(
        "zork-notice-v1:{}:{}:{key}",
        state.session_id, state.generation.number
    ));
    let id = format!("call_notice_{:x}", digest)[..44].to_owned();
    let mut call = message(TranscriptRole::Assistant, "", false);
    call.runtime_generated = true;
    call.tool_calls.push(super::wire::ProviderToolCall {
        tool_call_id: id.clone(),
        tool_name: super::tools::PROVIDER_CALL_NAME.into(),
        arguments: json!({
            "tool": "runtime.notice", "goal": "Receive runtime information",
            "action": "Read runtime notification", "arguments": {}
        }),
    });
    messages.push(call);
    let mut result = message(TranscriptRole::Tool, content, is_error);
    result.tool_call_id = Some(id);
    messages.push(result);
}

fn append_unpaired_delivery(
    messages: &mut Vec<ProviderMessage>,
    state: &SessionState,
    key: &str,
    delivery: &ToolDelivery,
) {
    match delivery {
        ToolDelivery::Result { invocation, result, .. } => append_notice(messages, state, key,
            &format!("A previously unfinished tool invocation has now returned. invocation_id={} tool={} result={}",
                invocation.invocation_id, invocation.tool, render_result(result)),
            result.outcome != ToolOutcome::Succeeded),
        ToolDelivery::Pending { invocation } => append_notice(messages, state, key,
            &format!("Tool invocation {} ({}) is still unfinished.", invocation.invocation_id, invocation.tool), false),
    }
    if let ToolDelivery::Result { result, .. } = delivery {
        if let Some(message) = messages.last_mut() {
            message.images = result.images.clone();
        }
    }
}

fn append_direct_delivery(messages: &mut Vec<ProviderMessage>, delivery: &ToolDelivery) {
    let (invocation, content, is_error) = match delivery {
        ToolDelivery::Pending { invocation } => (invocation,
            format!("Tool invocation is still unfinished and running. invocation_id={} tool={}. Its completion will be delivered automatically. Use this invocation_id with tool.cancel if interruption is needed; do not repeat the operation.", invocation.invocation_id, invocation.tool), false),
        ToolDelivery::Result { invocation, result, mode: ToolDeliveryMode::Direct } => {
            (invocation, render_result(result), result.outcome != ToolOutcome::Succeeded)
        }
        ToolDelivery::Result { .. } => return,
    };
    let mut result = message(TranscriptRole::Tool, &content, is_error);
    if let ToolDelivery::Result { result: data, .. } = delivery {
        result.images = data.images.clone();
    }
    result.tool_call_id = Some(invocation.provider_call_id.clone());
    messages.push(result);
}

fn tool_catalog_prompt(state: &SessionState) -> String {
    let mut text = String::from(
        "Runtime-generated `runtime.notice` call/result pairs carry notifications; they are transcript records, not tools you can execute. Logical tools are called through the single provider tool `call` with {\"tool\":\"complete.name\",\"action\":\"what this invocation does\",\"arguments\":{...}}. Write action concisely in the user's language for the activity UI; do not include private content, raw commands or reasoning. Action describes this call, without claiming success. Set optional top-level wait to your estimate of seconds until this tool is worth checking again. The shortest explicit wait in a batch controls when you can continue; it never cancels tools, and completion can resume you sooner. Use tool.help when you need a tool's current detailed usage.\n\nTools known at the start of this generation:\n",
    );
    for tool in &state.generation.tools {
        text.push_str(&format!(
            "- {} (version {}): {}\n",
            tool.name, tool.version, tool.description
        ));
    }
    text
}

fn render_result(result: &super::events::ToolResultData) -> String {
    serde_json::to_string(&json!({
        "invocation_id": result.invocation_id,
        "tool": result.tool,
        "outcome": result.outcome,
        "data": result.data,
    }))
    .unwrap_or_else(|_| format!("tool {} returned {:?}", result.tool, result.outcome))
}

fn message(role: TranscriptRole, content: &str, is_error: bool) -> ProviderMessage {
    ProviderMessage {
        images: Vec::new(),
        role,
        content: Arc::from(content),
        is_error,
        runtime_generated: false,
        tool_call_id: None,
        tool_calls: Vec::new(),
        provider_context: None,
    }
}
