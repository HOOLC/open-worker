//! Persist unresolved MCP handles in the ordinary dynamic-tool state.
use serde_json::{json, Value};
use zork_agent::session::{
    events::OutstandingItem,
    tools::{ToolCompatibility, ToolState},
};

pub struct McpState;
impl ToolCompatibility for McpState {
    fn state_namespace(&self) -> Option<&'static str> {
        Some("mcp")
    }
    fn migrate_result(&self, version: u32, value: Value) -> Result<Value, String> {
        if version == 2 {
            // New named calls complete through the ordinary executor. Keep
            // version 1 folding for historical receipts and explicit recovery.
            Ok(Value::Null)
        } else if version == 1 {
            Ok(value)
        } else {
            Err("Unsupported MCP result version".into())
        }
    }
    fn migrate_state(&self, state: ToolState) -> Result<ToolState, String> {
        if state.schema_version == 1 {
            Ok(state)
        } else {
            Err("Unsupported MCP state version".into())
        }
    }
    fn initial_state(&self) -> Option<ToolState> {
        Some(ToolState {
            schema_version: 1,
            value: json!({}),
        })
    }
    fn fold(&self, state: Option<&ToolState>, result: &Value) -> Result<Option<ToolState>, String> {
        let mut value = state.map(|s| s.value.clone()).unwrap_or(json!({}));
        if let Some(pending) = result["pending_delivery"].as_bool() {
            if pending {
                value["pending_delivery"] = json!("delivery_unknown");
            } else if let Some(map) = value.as_object_mut() {
                map.remove("pending_delivery");
            }
        }
        if let Some(calls) = result["calls"].as_array() {
            for call in calls {
                let folded = self.fold(
                    Some(&ToolState {
                        schema_version: 1,
                        value: value.clone(),
                    }),
                    call,
                )?;
                value = folded.ok_or("MCP state missing")?.value;
            }
        }
        if let (Some(id), Some(status)) = (result["call_id"].as_str(), result["state"].as_str()) {
            if matches!(
                status,
                "accepted" | "dispatching" | "running" | "cancel_requested" | "outcome_unknown"
            ) {
                value[id] = json!(status);
            } else if let Some(map) = value.as_object_mut() {
                map.remove(id);
            }
        }
        Ok(Some(ToolState {
            schema_version: 1,
            value,
        }))
    }
    fn outstanding(&self, state: Option<&ToolState>) -> Vec<OutstandingItem> {
        state.and_then(|s|s.value.as_object()).into_iter().flatten().map(|(id,status)|OutstandingItem{kind:"mcp".into(),id:id.clone(),summary:format!("MCP call {id}: {}. Use mcp recover for pending_delivery, otherwise mcp status; an unknown outcome must not be repeated.",status.as_str().unwrap_or("unknown"))}).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn handles_survive_replay_until_a_confirmed_terminal_result() {
        let initial = McpState
            .fold(None, &json!({"call_id":"call","state":"accepted"}))
            .unwrap();
        assert_eq!(McpState.outstanding(initial.as_ref()).len(), 1);
        let uncertain = McpState
            .fold(
                initial.as_ref(),
                &json!({"call_id":"call","state":"outcome_unknown"}),
            )
            .unwrap();
        assert_eq!(McpState.outstanding(uncertain.as_ref()).len(), 1);
        let read = McpState
            .fold(
                uncertain.as_ref(),
                &json!({"call_id":"call","data":"chunk"}),
            )
            .unwrap();
        assert_eq!(McpState.outstanding(read.as_ref()).len(), 1);
        let done = McpState
            .fold(
                read.as_ref(),
                &json!({"call_id":"call","state":"succeeded"}),
            )
            .unwrap();
        assert!(McpState.outstanding(done.as_ref()).is_empty());
    }
}

pub fn execution_for(op: &str, mut value: Value) -> zork_agent::session::tools::ToolExecution {
    if op == "inspect" {
        if let Some(items) = value["items"].as_array_mut() {
            for item in items {
                if let Some(definition) = item["definition"].as_object_mut() {
                    if let Some(schema) = definition.remove("inputSchema") {
                        definition.insert(
                            "parameters".into(),
                            Value::String(zork_agent::session::tools::parameter_types(&schema)),
                        );
                    }
                }
            }
        }
    }
    let mut result = execution(value);
    if matches!(op, "status" | "read" | "cancel" | "recover") {
        // Reading an unsuccessful operation is a successful query. Its state
        // remains visible and continues to drive the durable outstanding set.
        result.outcome = zork_agent::session::events::ToolOutcome::Succeeded;
    }
    result
}

pub fn execution(mut value: Value) -> zork_agent::session::tools::ToolExecution {
    use base64::Engine;
    use zork_agent::session::{events::ToolOutcome, tools::ToolExecution, wire::ToolImage};
    let mut images = Vec::new();
    if let Some(content) = value
        .pointer_mut("/result/content")
        .and_then(Value::as_array_mut)
    {
        for item in content {
            if item["type"] == "image" {
                if let (Some(mime), Some(data)) = (item["mimeType"].as_str(), item["data"].as_str())
                {
                    if matches!(
                        mime,
                        "image/png" | "image/jpeg" | "image/webp" | "image/gif"
                    ) && images.len() < 4
                        && base64::engine::general_purpose::STANDARD
                            .decode(data)
                            .is_ok()
                    {
                        images.push(ToolImage {
                            media_type: mime.into(),
                            base64: data.into(),
                        });
                        if let Some(object) = item.as_object_mut() {
                            object.remove("data");
                            object.insert("inline".into(), json!(true));
                        }
                    }
                }
            }
        }
    }
    let failed = value["pending_delivery"] == true
        || matches!(
            value["state"].as_str(),
            Some(
                "failed"
                    | "tool_error"
                    | "outcome_unknown"
                    | "not_dispatched"
                    | "expired"
                    | "result_unavailable"
            )
        );
    let mut execution = ToolExecution::success(value);
    execution.images = images;
    if failed {
        execution.outcome = ToolOutcome::Failed;
    }
    execution
}

#[cfg(test)]
mod result_tests {
    use super::*;
    use zork_agent::session::events::ToolOutcome;
    #[test]
    fn tool_errors_and_images_keep_their_native_meaning() {
        let result = execution(
            json!({"state":"succeeded","result":{"content":[{"type":"image","mimeType":"image/png","data":"aGVsbG8="}]}}),
        );
        assert_eq!(result.images.len(), 1);
        assert!(result.data["result"]["content"][0].get("data").is_none());
        assert_eq!(
            execution(json!({"state":"tool_error","result":{"isError":true}})).outcome,
            ToolOutcome::Failed
        );
    }
    #[test]
    fn lost_receipts_remain_outstanding_until_recovered() {
        let pending = McpState
            .fold(None, &json!({"pending_delivery":true}))
            .unwrap();
        assert_eq!(McpState.outstanding(pending.as_ref()).len(), 1);
        let recovered = McpState
            .fold(
                pending.as_ref(),
                &json!({"pending_delivery":false,"calls":[{"call_id":"id","state":"accepted"}]}),
            )
            .unwrap();
        let outstanding = McpState.outstanding(recovered.as_ref());
        assert_eq!(outstanding.len(), 1);
        assert_eq!(outstanding[0].id, "id");
    }
}

pub fn properties() -> Value {
    let string = || json!({"type":"string","minLength":1});
    let env = json!({"type":"object","additionalProperties":{"type":"string"},"description":"Environment variable map. Secret fields map to Gateway environment variable NAMES, never their secret values."});
    let grant = json!({"description":"Controls remote MCP callers only: local denies remote callers, mesh permits valid Mesh peers, selected permits the listed remote origin/agent pairs. Local Agents on the owning node can still use the service regardless of grant. This does not isolate local Agents or grant installation/management permission.","oneOf":[
        {"type":"object","properties":{"scope":{"enum":["local","mesh"]}},"required":["scope"],"additionalProperties":false},
        {"type":"object","properties":{"scope":{"const":"selected"},"subjects":{"type":"array","items":{"type":"object","properties":{"origin":string(),"agent":string()},"required":["origin","agent"],"additionalProperties":false}}},"required":["scope","subjects"],"additionalProperties":false}
    ]});
    json!({
        "op":{"type":"string","enum":["setup","installed","configure","install","probe","update","enable","disable","share","uninstall","search","inspect","call","status","cancel","read","recover"]},
        "owner":{"type":"string","description":"Target Gateway origin from setup. Omit for local install/installed; setup without owner discovers manageable Gateways."},
        "server_ref":{"type":"object","properties":{"owner_origin":string(),"server_id":string()},"required":["owner_origin","server_id"],"additionalProperties":false},
        "config":{"type":"object","properties":{
            "name":string(),"description":{"type":"string"},"enabled":{"type":"boolean","description":"False disables use for all callers while preserving installation. Disabled services remain in mcp.installed but are omitted from mcp.search."},"grant":grant.clone(),"tool_allowlist":{"type":["array","null"],"items":string(),"description":"Restricts callable tools for local and remote callers. Omit or use null to allow all; an empty array allows none."},
            "transport":{"oneOf":[
                {"type":"object","properties":{"kind":{"const":"http"},"url":string(),"secret_headers":env.clone()},"required":["kind","url"],"additionalProperties":false},
                {"type":"object","properties":{"kind":{"const":"stdio"},"command":string(),"args":{"type":"array","items":{"type":"string"}},"cwd":{"type":"string"},"env":env.clone(),"secret_env":env},"required":["kind","command"],"additionalProperties":false}
            ]}
        },"required":["name","transport","grant"],"additionalProperties":false},
        "expected_revision":string(),"grant":grant,
        "query":{"type":"string","maxLength":256},"tool":string(),"binding_revision":string(),"arguments":{"type":"object"},"call_id":string(),"cursor":string(),"offset":{"type":"integer","minimum":0}
    })
}
pub fn activity(args: &Value) -> zork_agent::session::tools::ToolActivity {
    use zork_agent::session::tools::ToolActivity;
    let (zh, en) = match args["op"].as_str() {
        Some("setup") => ("查看 MCP 安装环境", "Checking MCP setup"),
        Some("install") => ("安装 MCP", "Installing MCP"),
        Some("update" | "share") => ("配置 MCP", "Configuring MCP"),
        Some("enable") => ("启用 MCP", "Enabling MCP"),
        Some("disable") => ("停用 MCP", "Disabling MCP"),
        Some("uninstall") => ("卸载 MCP", "Uninstalling MCP"),
        Some("probe") => ("检测 MCP", "Checking MCP connection"),
        _ => ("访问 MCP", "Accessing MCP"),
    };
    ToolActivity::field(
        zh,
        en,
        args,
        if args.get("config").is_some() {
            "/config/name"
        } else {
            "/tool"
        },
    )
}

#[cfg(test)]
mod presentation_tests {
    use super::*;
    use zork_agent::session::events::ToolOutcome;
    #[test]
    fn inspect_converts_only_parameter_presentation_and_preserves_binding() {
        let raw = json!({"items":[{"binding_revision":"fixed", "definition":{
            "name":"echo","description":"Echo","inputSchema":{"type":"object","properties":{"text":{"type":"string","description":"Input text."}},"required":["text"],"additionalProperties":false}
        }}]});
        let result = execution_for("inspect", raw.clone());
        let definition = &result.data["items"][0]["definition"];
        assert!(definition.get("inputSchema").is_none());
        assert!(definition["parameters"]
            .as_str()
            .unwrap()
            .contains("// Input text.\n  text: string;"));
        assert_eq!(result.data["items"][0]["binding_revision"], "fixed");
        assert_eq!(execution_for("call", raw.clone()).data, raw);
    }
    #[test]
    fn querying_a_failed_operation_does_not_fail_the_query() {
        let value = json!({"call_id":"id","state":"tool_error","result":{"isError":true}});
        assert_eq!(
            execution_for("status", value.clone()).outcome,
            ToolOutcome::Succeeded
        );
        assert_eq!(execution_for("status", value.clone()).data, value);
        assert_eq!(execution_for("call", value).outcome, ToolOutcome::Failed);
    }
}
