//! Agent administration uses the authenticated node-management grant, never a
//! service invocation grant. Mutations and their receipts commit together.
use super::*;

pub fn is_mutation(op: &str) -> bool {
    matches!(
        op,
        "install" | "update" | "enable" | "disable" | "share" | "uninstall"
    )
}
pub fn is_operation(op: &str) -> bool {
    is_mutation(op) || matches!(op, "setup" | "installed" | "configure" | "probe")
}
pub fn receipt(value: &Value) -> Option<&str> {
    value["call_id"]
        .as_str()
        .or_else(|| value["operation_id"].as_str())
}
fn allowed(state: &AppState, who: &Subject, local: bool) -> Result<()> {
    crate::node_access::manage(state, who, local)
        .map_err(|_| anyhow::anyhow!("mcp_management_denied"))
}
use crate::node_access::executable;
fn prepare(state: &AppState, mut config: ServerInput) -> Result<ServerInput> {
    if let runtime::Transport::Stdio {
        command, cwd, env, ..
    } = &mut config.transport
    {
        *command = executable(command)
            .context("mcp_command_not_found_on_target")?
            .to_string_lossy()
            .into_owned();
        if cwd.is_empty() {
            let directory = state.config.data_root.join("mcp/workspace");
            std::fs::create_dir_all(&directory)?;
            *cwd = directory.canonicalize()?.to_string_lossy().into_owned();
        }
        ensure!(
            std::path::Path::new(cwd).is_dir(),
            "mcp_working_directory_not_found"
        );
        if let Ok(path) = std::env::var("PATH") {
            env.entry("PATH".into()).or_insert(path);
        }
    }
    config.validate()?;
    Ok(config)
}
fn setup(state: &AppState) -> Value {
    let environment = crate::node_access::environment(state);
    json!({
        "owner":own_origin(state),"name":zork_config::load_config(&state.config.data_root).map(|c|c.mesh.name).unwrap_or_default(),
        "os":std::env::consts::OS,"arch":std::env::consts::ARCH,"commands":environment["commands"],
        "operations":["installed","configure","install","probe","update","enable","disable","share","uninstall"],
        "instructions":"Manage MCP for the user's requested task. Supply owner for install and server_ref for existing services. HTTP config uses transport={kind:http,url,secret_headers?}; stdio uses transport={kind:stdio,command,args?,cwd?,env?,secret_env?}. Bare commands resolve on THIS Gateway, cwd defaults to its managed MCP workspace, and PATH defaults to this Gateway's PATH. Prepare missing packages with execution tools on this device, never on another device or by asking the user to run zork CLI. Pin package versions. Credential fields name Gateway environment variables; never put secret values in tool arguments. install registers configuration; use probe and inspect before claiming it works. Choose local/mesh/selected sharing explicitly. Read installed/configure for config_revision before update, enable, disable, share or uninstall. If delivery is unknown use recover instead of creating another operation."
    })
}
pub async fn targets(state: &AppState, who: &Subject) -> Result<Value> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let mut owners = vec![own_origin(state)];
    let config = zork_config::load_config(&state.config.data_root)?;
    if config.mesh.enabled {
        owners.extend(config.mesh.peers.iter().map(|p| p.origin.clone()));
    }
    owners.sort();
    owners.dedup();
    let results = stream::iter(owners.into_iter().map(|owner| {
        let subject = who.clone();
        async move {
            let request: Operation = serde_json::from_value(json!({"op":"setup"}))?;
            let result = tokio::time::timeout_at(
                deadline,
                route(
                    state,
                    &owner,
                    Rpc {
                        interrupt: false,
                        subject,
                        invocation_id: "setup".into(),
                        request,
                    },
                ),
            )
            .await;
            Ok::<_, anyhow::Error>((owner, result))
        }
    }))
    .buffer_unordered(4)
    .collect::<Vec<_>>()
    .await;
    let mut targets = Vec::new();
    let mut unavailable = Vec::new();
    for result in results {
        let (owner, result) = result?;
        match result {
            Ok(Ok(value)) => targets.push(value),
            Ok(Err(error)) => unavailable.push(json!({"owner":owner,"reason":safe_error(&error)})),
            Err(_) => unavailable.push(json!({"owner":owner,"reason":"mcp_timeout"})),
        }
    }
    targets.sort_by_key(|v| v["owner"].to_string());
    Ok(json!({"targets":targets,"unavailable_nodes":unavailable}))
}
pub async fn execute(state: &AppState, rpc: Rpc, local: bool) -> Result<Value> {
    allowed(state, &rpc.subject, local)?;
    let request = &rpc.request;
    if matches!(request.op.as_str(), "install" | "installed" | "setup") {
        ensure!(
            request
                .owner
                .as_ref()
                .is_none_or(|o| o == &own_origin(state) || (local && o == "local")),
            "mcp_wrong_owner"
        );
    } else {
        let reference = request.server_ref.as_ref().context("mcp_missing_server")?;
        ensure!(
            reference.owner_origin == own_origin(state)
                || (local && reference.owner_origin == "local"),
            "mcp_wrong_owner"
        );
        valid_id(&reference.server_id)?;
    }
    match request.op.as_str() {
        "setup"=>Ok(setup(state)),
        "installed"=>page(state.mcp.store.servers()?.iter().map(|s|json!({"server":state.mcp.descriptor(s,&own_origin(state)),"grant":s.config.grant,"enabled":s.config.enabled,"tool_allowlist":s.config.tool_allowlist})).collect(),&request.cursor),
        "configure"=>{
            let server=state.mcp.store.server(&request.server_ref.as_ref().context("mcp_missing_server")?.server_id)?;
            Ok(json!({"server_ref":{"owner_origin":own_origin(state),"server_id":server.id},"config_revision":server.revision,"config":server.config}))
        },
        "probe"=>{
            let mut probe=rpc.clone();probe.request.op="inspect".into();probe.request.tool=None;
            // Node managers may probe a local-only service; this does not grant
            // subsequent tool calls, which still pass the service grant.
            let result=Box::pin(super::execute(state,probe,true)).await?;
            allowed(state,&rpc.subject,local)?;
            Ok(result)
        },
        op if is_mutation(op)=>{
            let _policy=state.mcp.policy.lock().expect("mcp policy");
            allowed(state,&rpc.subject,local)?;
            // Resolve after checking for an existing receipt: a removed command
            // or changed PATH must not prevent recovery of a committed install.
            let fingerprint=digest(&(&rpc.subject,request))?;
            if let Some(result)=state.mcp.store.management_receipt(&rpc.subject,&rpc.invocation_id,&fingerprint)? {return Ok(result);}
            let config=request.config.clone().map(|c|prepare(state,c)).transpose()?;
            let result=state.mcp.store.manage(&rpc.subject,&rpc.invocation_id,&fingerprint,&own_origin(state),request,config)?;
            if let Some(id)=result["server_ref"]["server_id"].as_str(){state.mcp.invalidate(id);}
            Ok(result)
        },
        _=>anyhow::bail!("mcp_invalid_operation"),
    }
}

pub fn rejected(error: &anyhow::Error) -> bool {
    matches!(
        safe_error(error).as_str(),
        "mcp_management_denied"
            | "mcp_revision_conflict"
            | "mcp_command_not_found_on_target"
            | "mcp_working_directory_not_found"
            | "mcp_missing_config"
            | "mcp_missing_grant"
            | "mcp_invalid_name"
            | "mcp_config_limit"
            | "mcp_invalid_id"
            | "mcp_wrong_owner"
            | "mcp_https_required"
            | "mcp_reserved_header"
            | "mcp_invalid_endpoint"
            | "mcp_absolute_command_and_cwd_required"
            | "mcp_not_found"
            | "mcp_management_history_limit"
            | "mcp_server_limit"
    )
}
