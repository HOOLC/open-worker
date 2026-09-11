//! Named device/MCP/skill capabilities share targets and durable state groups.
use super::*;
use zork_agent::session::{events::OutstandingItem, tools::ToolState};

struct Named {
    base: String,
    http: reqwest::Client,
    name: String,
    fields: Vec<String>,
}
struct NodeState;
impl ToolCompatibility for NodeState {
    fn state_namespace(&self) -> Option<&'static str> {
        Some("device.exec")
    }
    fn migrate_result(&self, v: u32, data: Value) -> Result<Value, String> {
        if v == 2 {
            // Current calls use the executor's pending set. Version 1 results
            // still replay through the legacy receipt fold without data loss.
            Ok(Value::Null)
        } else if v == 1 {
            Ok(data)
        } else {
            Err("Unsupported node tool result".into())
        }
    }
    fn migrate_state(&self, state: ToolState) -> Result<ToolState, String> {
        if state.schema_version == 1 {
            Ok(state)
        } else {
            Err("Unsupported node tool state".into())
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
            } else {
                value
                    .as_object_mut()
                    .ok_or("Invalid node state")?
                    .remove("pending_delivery");
            }
        }
        if let Some(items) = result["operations"].as_array() {
            for item in items {
                value = self
                    .fold(
                        Some(&ToolState {
                            schema_version: 1,
                            value,
                        }),
                        item,
                    )?
                    .ok_or("Missing state")?
                    .value;
            }
        }
        if let (Some(id), Some(status)) =
            (result["operation_id"].as_str(), result["state"].as_str())
        {
            if matches!(
                status,
                "accepted" | "dispatching" | "running" | "outcome_unknown"
            ) {
                value[id] = json!({"state":status,"target":result["target"]});
            } else {
                value
                    .as_object_mut()
                    .ok_or("Invalid node state")?
                    .remove(id);
            }
        }
        Ok(Some(ToolState {
            schema_version: 1,
            value,
        }))
    }
    fn outstanding(&self, state: Option<&ToolState>) -> Vec<OutstandingItem> {
        state.and_then(|s|s.value.as_object()).into_iter().flatten().map(|(id,v)|OutstandingItem{kind:"device".into(),id:id.clone(),summary:format!("Node operation {id}: {v}. Use device.status or device.recover; unknown effects must not be repeated.")}).collect()
    }
}
fn add(
    registry: &Arc<ToolRegistry>,
    base: &str,
    name: &str,
    description: &str,
    properties: Value,
    required: Vec<&str>,
    http: &reqwest::Client,
) -> anyhow::Result<()> {
    let compatibility: Arc<dyn ToolCompatibility> = if name.starts_with("mcp.") {
        Arc::new(mcp::McpState)
    } else {
        Arc::new(NodeState)
    };
    let fields = properties
        .as_object()
        .expect("tool properties")
        .keys()
        .cloned()
        .collect();
    let owned = name.to_owned();
    let activity_name = owned.clone();
    registry.register(Arc::new(ToolInstance::new(ToolContract{name:owned.clone(),version:ToolVersion::new(if name.starts_with("mcp.") { "node-tools-5" } else { "node-tools-4" })?,initial_description:description.into(),detailed_description:format!("{description} target is the exact Gateway identity returned by device.list, not a display name. Omit target for this execution node. Identity and delivery deduplication come from ToolContext. Operations complete through ordinary tool completion events. Use tool.cancel with the invocation ID to interrupt pending work. Live output is available at .zork/live-<invocation_id>.log in this session workspace; read it with file.read. The completion result includes output_path. Do not repeat effects when the result says outcome_unknown."),input_schema:json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})},Arc::new(Named{base:base.into(),http:http.clone(),name:owned,fields}),compatibility)?.with_activity(move|args|activity(&activity_name,args)).advertise(!legacy(name))));
    Ok(())
}
pub fn register(
    registry: &Arc<ToolRegistry>,
    base: &str,
    http: &reqwest::Client,
) -> anyhow::Result<()> {
    let string = || json!({"type":"string","minLength":1});
    let target = json!({"target":string()});
    for (name,description,extra,required) in [
        ("device.list","Discover manageable Mesh Gateways, their identity, environment and connectivity. Use this before selecting a device.",json!({}),vec![]),
        ("device.inspect","Inspect the target Gateway's OS, commands and managed workspace location.",json!({}),vec![]),
        ("device.agents","List Agent IDs and names on the target node for skill binding.",json!({}),vec![]),
        ("device.exec","Execute the user's authorized command on the target device to prepare dependencies or files. Completes when the command exits; output is streamed to the ordinary live log. Do not background the command with &; execution is already asynchronous. cwd must be an absolute path on the TARGET device; omitted cwd is isolated by caller session. No automatic execution replay after Gateway restart.",json!({"command":{"type":"string","minLength":1,"maxLength":65536},"cwd":string(),"env":{"type":"object","additionalProperties":{"type":"string"}}}),vec!["command"]),
        ("device.status","Read an operation's state and result. A successful query can report a failed operation. cancelled/timed_out are terminal; result.process_state reports exited or not_started. Do not keep polling or recover terminal operations. Prior effects are not rolled back. Reuse its operation_id; target can be recovered from the caller's receipt.",json!({"operation_id":string()}),vec!["operation_id"]),
        ("device.read","Read captured command output in bounded pages using next_offset. Reading output succeeds even if the command failed or was cancelled. base64 preserves exact bytes; text is a display preview.",json!({"operation_id":string(),"offset":{"type":"integer","minimum":0}}),vec!["operation_id"]),
        ("device.cancel","Request interruption of an owned device operation. Poll device.status for cancelled; process_state=exited or not_started confirms no owned process remains, without undoing prior effects. outcome_unknown means termination could not be confirmed.",json!({"operation_id":string()}),vec!["operation_id"]),
        ("device.recover","Recover original device/skill operation receipts after a lost reply or caller restart.",json!({}),vec![]),
    ]{let mut props=target.clone();props.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());add(registry,base,name,description,props,required,http)?;}
    let package = json!({"type":"object","properties":{"content":string(),"resources":{"type":"array","maxItems":32,"items":{"type":"object","properties":{"path":string(),"base64":{"type":"string"},"executable":{"type":"boolean"}},"required":["path","base64"],"additionalProperties":false}}},"required":["content"],"additionalProperties":false});
    for (name,description,extra,required) in [
        ("skill.install","Install a complete skill manifest plus resource files on a target Gateway. Package JSON is limited to 96 KiB; resources use relative paths and base64. Installation does not automatically bind it to every Agent.",json!({"package":package}),vec!["package"]),
        ("skill.import","Import a prepared skill directory from the target device, including resource files. Hidden entries and symlinks are not imported. Use device.exec to fetch or prepare the directory first.",json!({"path":string()}),vec!["path"]),
        ("skill.installed","List managed skill packages and their current revisions on a target Gateway.",json!({"cursor":string()}),vec![]),
        ("skill.export","Read a complete managed skill package for inspection or transfer. expected_revision optionally pins the snapshot.",json!({"skill_id":string(),"expected_revision":string()}),vec!["skill_id"]),
        ("skill.share","Copy a specific skill revision and resources from target to destination Gateway. Both nodes must authorize management; installation on the destination does not bind it automatically.",json!({"skill_id":string(),"expected_revision":string(),"destination":string()}),vec!["skill_id","expected_revision","destination"]),
        ("skill.bindings","List Agents bound to a managed skill on its target node.",json!({"skill_id":string()}),vec!["skill_id"]),
        ("skill.bind","Bind a managed skill revision to an explicit Agent on the same target node; preserves its other skill sources. Use device.agents to select the Agent.",json!({"skill_id":string(),"expected_revision":string(),"agent_id":string()}),vec!["skill_id","expected_revision","agent_id"]),
        ("skill.unbind","Remove only this managed skill from the specified Agent's sources, preserving all other sources.",json!({"skill_id":string(),"expected_revision":string(),"agent_id":string()}),vec!["skill_id","expected_revision","agent_id"]),
        ("skill.uninstall","Archive an unbound managed skill and its resources. Inspect skill.bindings and unbind the selected Agents first. Files are preserved for recovery.",json!({"skill_id":string(),"expected_revision":string()}),vec!["skill_id","expected_revision"]),
    ]{let mut props=target.clone();props.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());add(registry,base,name,description,props,required,http)?;}
    let properties = mcp::properties();
    for (op,description,fields,required) in [
        ("setup","Compatibility MCP setup guide. Prefer device.list and device.inspect for selecting and preparing a node.",vec![],vec![]),
        ("installed","List MCP installations on a target node, including current configuration revisions.",vec!["cursor"],vec![]),
        ("configure","Inspect an MCP server configuration and credential references on its target node.",vec!["server_id"],vec!["server_id"]),
        ("install","Install MCP configuration on a selected device. First prepare missing dependencies with device.exec; then probe the installation. Choose sharing explicitly in config.grant. Grants restrict remote callers; local Agents on the owning node retain access.",vec!["config"],vec!["config"]),
        ("probe","Probe an installed MCP on its target node before claiming it is ready. This management check verifies connectivity, not permission to call tools; ordinary calls still enforce grant and tool_allowlist.",vec!["server_id"],vec!["server_id"]),
        ("update","Replace an MCP configuration using the current expected_revision.",vec!["server_id","config","expected_revision"],vec!["server_id","config","expected_revision"]),
        ("enable","Enable an MCP installation using its current expected_revision.",vec!["server_id","expected_revision"],vec!["server_id","expected_revision"]),
        ("disable","Disable an MCP installation using its current expected_revision.",vec!["server_id","expected_revision"],vec!["server_id","expected_revision"]),
        ("share","Change an MCP server's remote sharing grant using its current expected_revision. selected does not restrict local Agents on the owning node; this does not change management permission.",vec!["server_id","expected_revision","grant"],vec!["server_id","expected_revision","grant"]),
        ("uninstall","Uninstall an MCP server using its current expected_revision.",vec!["server_id","expected_revision"],vec!["server_id","expected_revision"]),
        ("search","Search usable MCP services across Mesh; optionally filter target. Results include target and server_id.",vec!["query","cursor"],vec![]),
        ("inspect","Read MCP tools or one specific tool's TypeScript parameter definition and binding_revision on the selected target/server_id. mcp_disabled means the installation is preserved but disabled; mcp_tool_not_allowed means its tool_allowlist excludes this tool; mcp_access_denied means caller authorization failed.",vec!["server_id","tool","cursor"],vec!["server_id"]),
        ("call","Call the inspected MCP tool with its binding_revision; completes with the actual MCP result. mcp_disabled means the server is disabled; mcp_tool_not_allowed means the tool is excluded; mcp_definition_changed means inspect the current definition before a new call. Only not_dispatched confirms no dispatch. An interrupted call may have effects even when the error identifies a policy change; uncertain outcomes must not be repeated.",vec!["server_id","tool","binding_revision","arguments"],vec!["server_id","tool","binding_revision","arguments"]),
        ("status","Read an MCP invocation state/result using its operation_id. Query success does not mean the invocation succeeded; inspect state/result.",vec!["operation_id"],vec!["operation_id"]),
        ("read","Read a large MCP result using operation_id and byte offset.",vec!["operation_id","offset"],vec!["operation_id"]),
        ("cancel","Request interruption of an MCP invocation using its operation_id.",vec!["operation_id"],vec!["operation_id"]),
        ("recover","Recover original MCP invocation or management receipts after lost delivery.",vec![],vec![]),
    ]{let mut props=target.clone();for field in fields{props[field]=properties.get(field).cloned().unwrap_or_else(string);}add(registry,base,&format!("mcp.{op}"),description,props,required,http)?;}
    Ok(())
}
fn legacy(name: &str) -> bool {
    matches!(
        name,
        "device.status"
            | "device.read"
            | "device.cancel"
            | "device.recover"
            | "mcp.status"
            | "mcp.read"
            | "mcp.cancel"
            | "mcp.recover"
            | "mcp.setup"
    )
}
impl ToolImplementation for Named {
    fn execute<'a>(
        &'a self,
        context: &'a ToolContext,
        args: &'a Value,
    ) -> Pin<Box<dyn Future<Output = ToolExecution> + Send + 'a>> {
        Box::pin(async move {
            let mut execution = self.execution(context, args, false).await;
            if !legacy(&self.name) {
                execution.result_schema_version = 2;
            }
            execution
        })
    }
    fn cancel<'a>(
        &'a self,
        context: &'a ToolContext,
        args: &'a Value,
    ) -> Pin<Box<dyn Future<Output = Option<ToolExecution>> + Send + 'a>> {
        Box::pin(async move {
            if self.mutation() {
                {
                    let mut result = self.execution(context, args, true).await;
                    result.result_schema_version = 2;
                    Some(result)
                }
            } else {
                None
            }
        })
    }
}
fn node_execution(name: &str, value: Value) -> ToolExecution {
    let query = matches!(
        name,
        "device.status" | "device.read" | "device.cancel" | "device.recover"
    );
    let failed = !query
        && (value["pending_delivery"] == true
            || matches!(
                value["state"].as_str(),
                Some("failed" | "outcome_unknown" | "not_dispatched")
            ));
    let mut result = ToolExecution::success(value);
    if failed {
        result.outcome = ToolOutcome::Failed;
    }
    result
}

impl Named {
    fn mutation(&self) -> bool {
        matches!(
            self.name.as_str(),
            "device.exec"
                | "skill.install"
                | "skill.import"
                | "skill.share"
                | "skill.bind"
                | "skill.unbind"
                | "skill.uninstall"
                | "mcp.call"
                | "mcp.install"
                | "mcp.update"
                | "mcp.enable"
                | "mcp.disable"
                | "mcp.share"
                | "mcp.uninstall"
        )
    }
    async fn execution(
        &self,
        context: &ToolContext,
        args: &Value,
        interrupt: bool,
    ) -> ToolExecution {
        let result = async {
            let value = self.request(context, args, interrupt).await?;
            if self.mutation() {
                self.complete(context, value, interrupt).await
            } else {
                Ok(value)
            }
        }
        .await;
        match result {
            Ok(mut value) => {
                if !legacy(&self.name) {
                    present_result(&self.name, &mut value);
                }
                if self.mutation() {
                    if let Some(result) = value["result"].as_object_mut() {
                        result.remove("read_with");
                    }
                    if let Some(object) = value.as_object_mut() {
                        object.remove("operation_id");
                        object.remove("call_id");
                        object.remove("pending_delivery");
                    }
                }
                let mut execution = if self.name.starts_with("mcp.") {
                    mcp::execution_for(self.name.trim_start_matches("mcp."), value)
                } else {
                    node_execution(&self.name, value)
                };
                if self.mutation() && execution.data["state"] == "cancelled" {
                    execution.outcome = ToolOutcome::Cancelled;
                }
                if self.name == "mcp.call"
                    && serde_json::to_vec(&execution.data)
                        .is_ok_and(|bytes| bytes.len() > 64 * 1024)
                {
                    execution.data["result"] =
                        json!({"summary":"Full result is in output_path; use file.read."});
                }
                execution
            }
            Err(error) if error.is::<DeliveryRejected>() => ToolExecution {
                images: vec![],
                outcome: ToolOutcome::Failed,
                data: json!({"state":"not_dispatched","effects_may_have_occurred":false,"error":error.to_string()}),
                result_schema_version: 1,
                knowledge: None,
            },
            Err(error) => ToolExecution {
                images: vec![],
                outcome: ToolOutcome::Failed,
                data: if self.mutation() {
                    json!({"state":"outcome_unknown","error":error.to_string(),"effects_may_have_occurred":true,"instruction":"Do not repeat this operation; its outcome could not be confirmed."})
                } else {
                    json!({"error":error.to_string()})
                },
                result_schema_version: 1,
                knowledge: None,
            },
        }
    }
    async fn request(
        &self,
        context: &ToolContext,
        args: &Value,
        interrupt: bool,
    ) -> anyhow::Result<Value> {
        if !interrupt {
            if let Some(args) = args.as_object() {
                if let Some(field) = args.keys().find(|field| !self.fields.contains(field)) {
                    return Err(DeliveryRejected(format!("Invalid argument `{field}` for {}. Allowed fields: {}. Use tool.help for the parameter definition.", self.name, self.fields.join(", "))).into());
                }
            }
        }
        let (url, body) = if let Some(op) = self.name.strip_prefix("mcp.") {
            let mut request = args.clone();
            request["op"] = json!(op);
            let map = request
                .as_object_mut()
                .ok_or_else(|| anyhow::anyhow!("Invalid MCP arguments"))?;
            let target = map.remove("target");
            if let Some(server) = map.remove("server_id") {
                map.insert("server_ref".into(),json!({"owner_origin":target.clone().unwrap_or(json!("local")),"server_id":server}));
            }
            if let Some(target) = target {
                map.insert("owner".into(), target);
            }
            if let Some(id) = map.remove("operation_id") {
                map.insert("call_id".into(), id);
            }
            (
                "/v1/mcp",
                json!({"session_id":context.session_id,"invocation_id":context.invocation_id,"request":request}),
            )
        } else {
            (
                "/v1/node-tools",
                json!({"session_id":context.session_id,"invocation_id":context.invocation_id,"tool":self.name,"arguments":args}),
            )
        };
        let suffix = if interrupt { "/interrupt" } else { "" };
        let mut attempt = 0;
        let mut value = loop {
            let result: anyhow::Result<Value> = async {
                let response = self
                    .http
                    .post(format!("{}{url}{suffix}", self.base))
                    .json(&body)
                    .send()
                    .await?;
                let status = response.status();
                let text = response.text().await?;
                if self.name.starts_with("mcp.") && status == reqwest::StatusCode::UNPROCESSABLE_ENTITY {
                    return Err(DeliveryRejected(format!("Invalid arguments for {}. Check required fields and value types with tool.help. Allowed fields: {}.", self.name, self.fields.join(", "))).into());
                }
                decode_response(status, &text)
            }
            .await;
            let retry = matches!(&result,Ok(value) if value["pending_delivery"] == true)
                || result
                    .as_ref()
                    .err()
                    .is_some_and(|error| error.downcast_ref::<reqwest::Error>().is_some());
            if !self.mutation() || !retry || attempt == 3 {
                break result?;
            }
            tokio::time::sleep(Duration::from_millis(250 << attempt)).await;
            attempt += 1;
        };
        if self.name.starts_with("mcp.") {
            normalize_mcp(&mut value);
            if let Some(target) = args.get("target") {
                if value.get("target").is_none() {
                    value["target"] = target.clone();
                }
            }
        }
        Ok(value)
    }
    async fn complete(
        &self,
        context: &ToolContext,
        receipt: Value,
        interrupt: bool,
    ) -> anyhow::Result<Value> {
        use base64::Engine;
        use std::io::{Read, Seek, SeekFrom, Write};
        if self.name.starts_with("mcp.") && self.name != "mcp.call" {
            return Ok(receipt);
        }
        let Some(id) = receipt["operation_id"]
            .as_str()
            .or(receipt["call_id"].as_str())
        else {
            anyhow::ensure!(
                receipt["pending_delivery"] != true,
                "Tool delivery could not be confirmed"
            );
            return Ok(receipt);
        };
        let domain = if self.name.starts_with("mcp.") {
            "mcp"
        } else {
            "device"
        };
        anyhow::ensure!(
            !context.invocation_id.contains(['/', '\\']),
            "Invalid invocation ID"
        );
        let relative = format!(".zork/live-{}.log", context.invocation_id);
        let path = std::path::Path::new(&context.workspace).join(&relative);
        std::fs::create_dir_all(
            path.parent()
                .ok_or_else(|| anyhow::anyhow!("Invalid output path"))?,
        )?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(!interrupt)
            .open(&path)?;
        let mut offset = if interrupt { file.metadata()?.len() } else { 0 };
        file.seek(SeekFrom::Start(offset))?;
        for attempt in 0..4 {
            let result: anyhow::Result<Value> = async {
                let mut response = self
            .http
            .post(format!("{}/v1/tools/watch", self.base))
            .json(&json!({"session_id":context.session_id,"domain":domain,"id":id,"offset":offset}))
            .send()
            .await?
            .error_for_status()?;
                let mut buffer = Vec::new();
                while let Some(chunk) = response.chunk().await? {
                    buffer.extend_from_slice(&chunk);
                    anyhow::ensure!(
                        buffer.len() <= 16 * 1024 * 1024,
                        "Tool stream frame too large"
                    );
                    while let Some(end) = buffer.iter().position(|byte| *byte == b'\n') {
                        let event: Value = serde_json::from_slice(&buffer[..end])?;
                        buffer.drain(..=end);
                        anyhow::ensure!(
                            event.get("error").is_none(),
                            "Tool stream failed: {}",
                            event["error"]
                        );
                        let mut value = event["value"].clone();
                        if let Some(encoded) = value["chunk"]["base64"].as_str() {
                            anyhow::ensure!(
                                value["chunk"]["offset"].as_u64() == Some(offset),
                                "Tool output offset mismatch"
                            );
                            let bytes =
                                base64::engine::general_purpose::STANDARD.decode(encoded)?;
                            file.write_all(&bytes)?;
                            file.flush()?;
                            offset += bytes.len() as u64;
                        }
                        if event["done"] == true {
                            if domain == "mcp" {
                                file.seek(SeekFrom::Start(0))?;
                                value["result"] = serde_json::from_reader(&mut file)?;
                            }
                            file.seek(SeekFrom::End(
                                -(file.metadata()?.len().min(64 * 1024) as i64),
                            ))?;
                            let mut tail = Vec::new();
                            file.read_to_end(&mut tail)?;
                            value
                                .as_object_mut()
                                .ok_or_else(|| anyhow::anyhow!("Invalid tool completion"))?
                                .remove("chunk");
                            value["output_path"] = json!(relative);
                            if domain == "device" {
                                value["output"] = json!(String::from_utf8_lossy(&tail));
                            }
                            return Ok(value);
                        }
                    }
                }
                anyhow::bail!("Tool stream closed before completion")
            }
            .await;
            match result {
                Ok(value) => return Ok(value),
                Err(error) if attempt == 3 => return Err(error),
                Err(_) => tokio::time::sleep(Duration::from_millis(250 << attempt)).await,
            }
        }
        unreachable!()
    }
}
#[derive(Debug)]
struct DeliveryRejected(String);
impl std::fmt::Display for DeliveryRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for DeliveryRejected {}
pub(super) fn decode_response(status: reqwest::StatusCode, text: &str) -> anyhow::Result<Value> {
    let value = serde_json::from_str::<Value>(text);
    if !status.is_success() {
        let detail = value
            .as_ref()
            .ok()
            .and_then(|v| v["error"].as_str())
            .unwrap_or(text);
        let detail: String = detail.chars().take(4096).collect();
        if value
            .as_ref()
            .is_ok_and(|value| value["delivery_rejected"] == true)
        {
            return Err(DeliveryRejected(detail).into());
        }
        anyhow::bail!("Node tool failed ({status}): {detail}");
    }
    Ok(value?)
}

fn present_result(name: &str, value: &mut Value) {
    if value.get("tool").is_some() {
        value["tool"] = json!(name);
    }
    if name.starts_with("mcp.") {
        normalize_mcp(value);
        fn strip(value: &mut Value) {
            if let Some(object) = value.as_object_mut() {
                object.remove("server_ref");
                object.remove("call_id");
            }
            if let Some(server) = value.get_mut("server") {
                strip(server);
            }
            for field in ["items", "calls"] {
                if let Some(items) = value.get_mut(field).and_then(Value::as_array_mut) {
                    for item in items {
                        strip(item);
                    }
                }
            }
        }
        strip(value);
    }
}

fn normalize_mcp(value: &mut Value) {
    if let Some(reference) = value.get("server_ref").cloned() {
        value["target"] = reference["owner_origin"].clone();
        value["server_id"] = reference["server_id"].clone();
    }
    if let Some(id) = value.get("call_id").cloned() {
        value["operation_id"] = id;
    }
    if let Some(server) = value.get_mut("server") {
        normalize_mcp(server);
    }
    for field in ["items", "calls"] {
        if let Some(items) = value.get_mut(field).and_then(Value::as_array_mut) {
            for item in items {
                normalize_mcp(item);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zork_agent::session::{
        events::{SessionEvent, ToolResultData},
        state::SessionState,
    };
    #[test]
    fn rendered_mcp_help_explains_local_access_and_tool_limits() {
        let schema = mcp::properties();
        for field in ["config", "grant"] {
            let text = zork_agent::session::tools::parameter_types(&schema[field]);
            assert!(text.contains("Local Agents on the owning node"), "{text}");
            assert!(text.contains("remote"), "{text}");
        }
        let text = zork_agent::session::tools::parameter_types(&schema["config"]);
        assert!(text.contains("empty array allows none"), "{text}");
        assert!(text.contains("omitted from mcp.search"), "{text}");
    }
    #[test]
    fn public_results_preserve_call_identity_and_hide_transport_aliases() {
        let mut shared =
            json!({"tool":"skill.install","state":"succeeded","result":{"skill_id":"copy"}});
        present_result("skill.share", &mut shared);
        assert_eq!(shared["tool"], "skill.share");
        assert_eq!(shared["result"]["skill_id"], "copy");
        let mut searched = json!({"items":[{"server_ref":{"owner_origin":"node","server_id":"server"},"name":"echo"}]});
        present_result("mcp.search", &mut searched);
        assert_eq!(
            searched,
            json!({"items":[{"target":"node","server_id":"server","name":"echo"}]})
        );
        let mut installed = json!({"items":[{"server":{"server_ref":{"owner_origin":"node","server_id":"server"},"availability":"disabled"},"enabled":false}]});
        present_result("mcp.installed", &mut installed);
        assert_eq!(
            installed,
            json!({"items":[{"server":{"target":"node","server_id":"server","availability":"disabled"},"enabled":false}]})
        );
    }

    #[tokio::test]
    async fn invalid_public_field_is_rejected_before_transport() {
        let tool = Named {
            base: "http://127.0.0.1:1".into(),
            http: reqwest::Client::new(),
            name: "mcp.inspect".into(),
            fields: vec!["target".into(), "server_id".into(), "tool".into()],
        };
        let context = ToolContext {
            control: None,
            session_id: "test".into(),
            invocation_id: "invalid-field".into(),
            workspace: "/unused".into(),
        };
        let result = tool
            .execute(&context, &json!({"server_id":"server","tool_name":"echo"}))
            .await;
        assert_eq!(result.outcome, ToolOutcome::Failed);
        assert_eq!(result.data["state"], "not_dispatched");
        let error = result.data["error"].as_str().unwrap();
        assert!(error.contains("tool_name") && error.contains("target, server_id, tool"));
        assert!(!error.contains("server_ref") && !error.contains("call_id"));
    }
    #[test]
    fn completed_cancellation_settles_and_queries_preserve_the_operation_outcome() {
        let running = json!({"operation_id":"job","state":"running"});
        let state = NodeState.fold(None, &running).unwrap();
        let cancelled = json!({"operation_id":"job","state":"cancelled","result":{"process_state":"exited","effects_may_have_occurred":true}});
        let done = NodeState.fold(state.as_ref(), &cancelled).unwrap();
        assert!(NodeState.outstanding(done.as_ref()).is_empty());
        for name in ["device.status", "device.read", "device.cancel"] {
            let unknown =
                json!({"operation_id":"job","state":"outcome_unknown","text":"captured output"});
            assert_eq!(
                node_execution(name, unknown.clone()).outcome,
                ToolOutcome::Succeeded
            );
            assert_eq!(node_execution(name, unknown.clone()).data, unknown);
            let unresolved = NodeState.fold(None, &unknown).unwrap();
            assert_eq!(NodeState.outstanding(unresolved.as_ref()).len(), 1);
        }
    }

    #[test]
    fn parameter_rejections_preserve_plain_text_and_json_diagnostics() {
        let plain =
            "request.config.grant: invalid type: string, expected internally tagged enum Grant";
        let error = decode_response(reqwest::StatusCode::UNPROCESSABLE_ENTITY, plain)
            .unwrap_err()
            .to_string();
        assert!(error.contains(plain));
        let error = decode_response(
            reqwest::StatusCode::BAD_REQUEST,
            r#"{"error":"mcp_revision_conflict"}"#,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("mcp_revision_conflict"));
        let huge = "字".repeat(10000);
        assert!(
            decode_response(reqwest::StatusCode::BAD_REQUEST, &huge)
                .unwrap_err()
                .to_string()
                .chars()
                .count()
                < 4200
        );
    }

    #[test]
    fn named_actions_share_pending_state_across_snapshot_replay() {
        let registry = Arc::new(ToolRegistry::default());
        super::super::register(&registry, "http://127.0.0.1:9".into()).unwrap();
        for (start, finish, key, id_field) in [
            (
                "device.exec",
                "device.status",
                "device.exec",
                "operation_id",
            ),
            (
                "skill.install",
                "device.status",
                "device.exec",
                "operation_id",
            ),
            ("mcp.call", "mcp.status", "mcp", "call_id"),
        ] {
            let mut state = SessionState::empty("test");
            state.created_at_ms = Some(0);
            let event = |tool: &str, status: &str| SessionEvent::ToolResult {
                result: ToolResultData {
                    images: vec![],
                    invocation_id: format!("{tool}-result"),
                    tool: tool.into(),
                    outcome: ToolOutcome::Succeeded,
                    data: json!({id_field:"01ARZ3NDEKTSV4RRFFQ69G5FAV","state":status}),
                    result_schema_version: 1,
                    knowledge: None,
                    finished_at_ms: 1,
                },
            };
            state.apply(&event(start, "accepted"), &registry).unwrap();
            assert_eq!(state.outstanding(&registry).len(), 1);
            let mut restored: SessionState =
                serde_json::from_value(serde_json::to_value(&state).unwrap()).unwrap();
            restored
                .apply(&event(finish, "succeeded"), &registry)
                .unwrap();
            assert!(restored.outstanding(&registry).is_empty());
            assert_eq!(restored.tool_states.len(), 1);
            assert!(restored.tool_states.contains_key(key));
        }
    }
}

fn activity(name: &str, args: &Value) -> ToolActivity {
    if let Some(op) = name.strip_prefix("mcp.") {
        let mut args = args.clone();
        args["op"] = json!(op);
        return mcp::activity(&args);
    }
    let (zh, en) = match name {
        "device.list" => ("查看设备", "Listing devices"),
        "device.inspect" => ("查看设备环境", "Inspecting device"),
        "device.agents" => ("查看设备伙伴", "Listing device agents"),
        "device.exec" => ("执行设备任务", "Executing device task"),
        "device.status" => ("查看任务状态", "Checking operation"),
        "device.read" => ("读取任务输出", "Reading output"),
        "device.cancel" => ("停止设备任务", "Stopping operation"),
        "device.recover" => ("恢复设备任务", "Recovering operations"),
        "skill.install" => ("安装技能", "Installing skill"),
        "skill.import" => ("导入技能", "Importing skill"),
        "skill.share" => ("共享技能", "Sharing skill"),
        "skill.bind" => ("绑定技能", "Binding skill"),
        "skill.unbind" => ("解绑技能", "Unbinding skill"),
        "skill.uninstall" => ("移除技能", "Removing skill"),
        _ => ("查看技能", "Inspecting skill"),
    };
    ToolActivity::new(zh, en, "")
}

#[cfg(test)]
mod stream_tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn new_catalog_has_one_interface_and_runtime_owned_request_identity() {
        let registry = Arc::new(ToolRegistry::default());
        super::super::register(&registry, "http://127.0.0.1:1".into()).unwrap();
        let names = registry
            .initial_catalog()
            .into_iter()
            .map(|entry| entry.name)
            .collect::<Vec<_>>();
        for name in [
            "mcp",
            "service",
            "mcp.setup",
            "mcp.status",
            "mcp.read",
            "mcp.cancel",
            "mcp.recover",
            "device.status",
            "device.read",
            "device.cancel",
            "device.recover",
            "agent.tasks",
            "chat.notify",
        ] {
            assert!(!names.iter().any(|item| item == name), "{name}");
            assert!(
                registry.current_contract(name).is_some(),
                "compatibility lost: {name}"
            );
        }
        assert!(registry.current_contract("device.jobs").is_none());
        assert!(!names.iter().any(|name| name == "device.jobs"));
        for name in [
            "device.exec",
            "mcp.call",
            "service.start",
            "service.inspect",
            "agent.list",
            "agent.message",
            "chat.send",
            "chat.preferences",
            "notify",
            "skill.install",
        ] {
            assert!(names.iter().any(|item| item == name), "{name}");
        }
        for name in [
            "browser",
            "service.start",
            "service.attach",
            "service.restart",
            "service.stop",
            "agent.assign",
            "agent.rework",
            "agent.message",
            "chat.send",
            "notify",
        ] {
            let schema = registry.current_contract(name).unwrap().input_schema;
            assert!(schema["properties"].get("request_id").is_none(), "{name}");
            assert!(
                !schema["required"]
                    .as_array()
                    .unwrap()
                    .contains(&json!("request_id")),
                "{name}"
            );
        }
        assert!(registry
            .current_contract("device.exec")
            .unwrap()
            .input_schema["properties"]
            .get("timeout_seconds")
            .is_none());
    }

    #[test]
    fn current_business_queries_do_not_recreate_legacy_pending_state() {
        for compatibility in [
            Arc::new(NodeState) as Arc<dyn ToolCompatibility>,
            Arc::new(mcp::McpState),
        ] {
            let old = ToolState {
                schema_version: 1,
                value: json!({"historical":"outcome_unknown"}),
            };
            let query = json!({"operations":[{"operation_id":"new","state":"running"}],"calls":[{"call_id":"new","state":"running"}]});
            let migrated = compatibility.migrate_result(2, query).unwrap();
            assert_eq!(
                compatibility.fold(Some(&old), &migrated).unwrap(),
                Some(old)
            );
            assert!(compatibility
                .outstanding(compatibility.fold(None, &migrated).unwrap().as_ref())
                .is_empty());
        }
    }

    #[tokio::test]
    async fn disconnected_output_resumes_at_committed_offset_without_reexecuting() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            for index in 0..3 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let header_end = loop {
                    let mut byte = [0];
                    socket.read_exact(&mut byte).await.unwrap();
                    request.push(byte[0]);
                    if request.ends_with(b"\r\n\r\n") {
                        break request.len();
                    }
                };
                let headers = String::from_utf8_lossy(&request).to_string();
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|value| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap();
                request.resize(header_end + length, 0);
                socket.read_exact(&mut request[header_end..]).await.unwrap();
                let body: Value = serde_json::from_slice(&request[header_end..]).unwrap();
                let response = if index == 0 {
                    assert!(headers.starts_with("POST /v1/node-tools "));
                    assert_eq!(body["invocation_id"], "resume-test");
                    json!({"operation_id":"receipt","state":"running"}).to_string()
                } else {
                    assert!(headers.starts_with("POST /v1/tools/watch "));
                    assert_eq!(body["id"], "receipt");
                    let (offset, next, text, done) = if index == 1 {
                        (0, 6, "Zmlyc3QK", false)
                    } else {
                        (6, 13, "c2Vjb25kCg==", true)
                    };
                    assert_eq!(body["offset"], offset);
                    format!(
                        "{}\n",
                        json!({"value":{"operation_id":"receipt","state":if done {"succeeded"}else{"running"},"result":{"process_state":if done {"exited"} else {"running"}},"chunk":{"offset":offset,"next_offset":next,"base64":text}},"done":done})
                    )
                };
                socket
                    .write_all(
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            response.len(),
                            response
                        )
                        .as_bytes(),
                    )
                    .await
                    .unwrap();
            }
        });
        let workspace = std::env::temp_dir().join(format!("zork-stream-{}", ulid::Ulid::new()));
        std::fs::create_dir_all(&workspace).unwrap();
        let tool = Named {
            base,
            http: reqwest::Client::new(),
            name: "device.exec".into(),
            fields: vec!["command".into()],
        };
        let context = ToolContext {
            control: None,
            session_id: "session".into(),
            invocation_id: "resume-test".into(),
            workspace: workspace.to_string_lossy().into(),
        };
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            tool.execute(&context, &json!({"command":"original"})),
        )
        .await
        .unwrap();
        server.await.unwrap();
        assert_eq!(result.outcome, ToolOutcome::Succeeded);
        assert_eq!(result.data["output"], "first\nsecond\n");
        assert_eq!(
            std::fs::read(workspace.join(result.data["output_path"].as_str().unwrap())).unwrap(),
            b"first\nsecond\n"
        );
        assert!(result.data.get("operation_id").is_none());
        assert!(NodeState
            .outstanding(NodeState.fold(None, &result.data).unwrap().as_ref())
            .is_empty());
        std::fs::remove_dir_all(workspace).unwrap();
    }
}
