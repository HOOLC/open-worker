use super::*;
use crate::db::{
    agents::{AgentRole, NodeAgent},
    chats::configuration_revision,
};
use api::Rpc;
use futures_util::{stream, StreamExt};
use std::time::Duration;

pub(super) async fn discover(state: &AppState, who: &Subject, args: &Value) -> Result<Value> {
    let targets = node_access::targets(state)?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    let replies = stream::iter(targets.into_iter().map(|target| {
        let rpc = Rpc {
            subject: who.clone(),
            invocation_id: "discover".into(),
            tool: "agent.list".into(),
            arguments: args.clone(),
            prepared_key: String::new(),
            files: vec![],
            interrupt: false,
        };
        async move {
            let result = tokio::time::timeout_at(deadline, api::route(state, &target, rpc)).await;
            (target, result)
        }
    }))
    .buffer_unordered(4)
    .collect::<Vec<_>>()
    .await;
    let mut nodes = Vec::new();
    let mut unavailable = Vec::new();
    for (target, result) in replies {
        match result {
            Ok(Ok(mut value)) if value["status"] != "rejected" => {
                value["target"] = json!(target);
                nodes.push(value);
            }
            Ok(Ok(value)) => unavailable.push(json!({"target":target,"error":value["error"]})),
            Ok(Err(error)) => {
                unavailable.push(json!({"target":target,"error":super::error(&error)}))
            }
            Err(_) => unavailable.push(json!({"target":target,"error":"node_timeout"})),
        }
    }
    nodes.sort_by_key(|value| value["target"].to_string());
    Ok(json!({"nodes":nodes,"unavailable_nodes":unavailable}))
}

fn summary(agent: &NodeAgent, manageable: bool) -> Result<Value> {
    Ok(
        json!({"id":agent.id,"name":agent.name,"avatar":agent.avatar,"manageable":manageable,"revision":configuration_revision(agent)?}),
    )
}
pub(super) async fn execute(
    state: &AppState,
    rpc: &Rpc,
    key: &str,
    object: &str,
    is_local: bool,
) -> Result<Value> {
    let args = &rpc.arguments;
    let manageable = node_access::manage(state, &rpc.subject, is_local).is_ok();
    match rpc.tool.as_str() {
        "agent.list" => {
            let scope = fingerprint(&("agents", args["query"].as_str()))?;
            let after = api::cursor(args, &scope)?;
            let query = args["query"].as_str().unwrap_or("").to_lowercase();
            let mut agents = state.db.node_agents()?;
            agents.sort_by(|a, b| a.id.cmp(&b.id));
            let agents = agents
                .into_iter()
                .filter(|a| {
                    after.as_ref().is_none_or(|p| a.id > *p)
                        && a.name.to_lowercase().contains(&query)
                })
                .take(api::limit(args))
                .collect::<Vec<_>>();
            let next = agents
                .last()
                .filter(|_| agents.len() == api::limit(args))
                .map(|a| api::encode_cursor(&scope, &a.id));
            Ok(
                json!({"items":agents.iter().map(|a|summary(a,manageable)).collect::<Result<Vec<_>>>()?,"next_cursor":next}),
            )
        }
        "agent.options" => {
            node_access::manage(state, &rpc.subject, is_local)?;
            let profiles = crate::agent::list_profiles(&state.agent).await?;
            let after = api::cursor(args, "agent-options")?
                .map(|s| s.parse::<usize>())
                .transpose()
                .context("invalid_chat_cursor")?
                .unwrap_or(0);
            let options=profiles.iter().filter(|p|p.auth_configured).flat_map(|p|p.models.iter().filter(|m|m.enabled&&m.limits.is_some()).map(move|m|json!({"profile_id":p.profile_id,"model":m.id,"thinking":m.thinking,"default_thinking":m.default_thinking}))).collect::<Vec<_>>();
            ensure!(after <= options.len(), "invalid_chat_cursor");
            let through = (after + api::limit(args)).min(options.len());
            Ok(
                json!({"items":&options[after..through],"next_cursor":(through<options.len()).then(||api::encode_cursor("agent-options",&through.to_string()))}),
            )
        }
        "agent.inspect" => {
            let id = field(args, "agent_id")?;
            let agent = state.db.node_agent(id)?.context("agent_not_found")?;
            let mut value = summary(&agent, manageable)?;
            if manageable {
                value["config"] = json!({"name":agent.name,"avatar":agent.avatar,"selection":{"profile_id":agent.profile_id,"model":agent.model,"thinking":agent.thinking},"instructions":agent.instructions,"skill_paths":agent.skill_paths});
                let mut sessions = Vec::new();
                for session in state.db.channel_agent_sessions(id)? {
                    if state.agent.service.contains(&session) {
                        let snapshot = state.agent.session_snapshot(&session).await?;
                        sessions.push(json!({"session_id":session,"status":snapshot.execution.status,"run":snapshot.execution.active_turn}));
                    }
                }
                value["sessions"] = json!(sessions);
            }
            Ok(value)
        }
        "agent.create" | "agent.update" => {
            node_access::manage(state, &rpc.subject, is_local)?;
            let creating = rpc.tool == "agent.create";
            let id = if creating {
                object
            } else {
                field(args, "agent_id")?
            };
            let _guard = state.entries.lock_local_task(&format!("agent:{id}")).await;
            let mut agent = if creating {
                NodeAgent {
                    id: id.into(),
                    name: String::new(),
                    avatar: None,
                    role: AgentRole::Leader,
                    profile_id: String::new(),
                    model: String::new(),
                    thinking: String::new(),
                    instructions: String::new(),
                    skill_paths: vec![],
                    allowed_leaders: vec![],
                    session_key: None,
                    session_id: None,
                }
            } else {
                state.db.node_agent(id)?.context("agent_not_found")?
            };
            let fields = if creating {
                &args["config"]
            } else {
                &args["changes"]
            };
            apply_configuration(state, &mut agent, fields).await?;
            node_access::manage(state, &rpc.subject, is_local)?;
            state.db.save_channel_agent(
                key,
                &agent,
                if creating {
                    None
                } else {
                    Some(field(args, "expected_revision")?)
                },
            )
        }
        "agent.interrupt" => {
            node_access::manage(state, &rpc.subject, is_local)?;
            let id = field(args, "agent_id")?;
            let session = field(args, "session_id")?;
            let run = field(args, "run_id")?;
            ensure!(
                state
                    .db
                    .channel_agent_sessions(id)?
                    .iter()
                    .any(|s| s == session),
                "agent_run_not_owned"
            );
            let events = state
                .agent
                .events(
                    session.into(),
                    None,
                    zork_agent_api::EventQuery { transient: false },
                )
                .await?;
            futures_util::pin_mut!(events);
            let current = state.agent.session_snapshot(session).await?;
            if current
                .execution
                .active_turn
                .as_ref()
                .is_some_and(|t| t.turn_id == run)
            {
                state
                    .agent
                    .cancel_session_run(session.into(), run.into())
                    .await?;
            } else {
                let value = json!({"state":"already_ended","run_id":run});
                state.db.finish_chat_outgoing(key, &value)?;
                return Ok(value);
            }
            let confirmed = if rpc.subject.session == session {
                false
            } else {
                tokio::time::timeout(Duration::from_secs(15),async{
                    while let Some(event)=events.next().await{
                        use zork_agent::session::{service::LiveSessionEvent,events::SessionEvent};
                        match event?{
                            LiveSessionEvent::Durable(e) if matches!(&e.event,SessionEvent::TurnFinished{turn_id,..} if turn_id==run)=>return Ok::<_,anyhow::Error>(true),
                            LiveSessionEvent::Snapshot(s) if s.execution.active_turn.as_ref().is_none_or(|t|t.turn_id!=run)=>return Ok(true),
                            LiveSessionEvent::Overview(s) if s.execution.active_turn.as_ref().is_none_or(|t|t.turn_id!=run)=>return Ok(true),
                            _=>{}
                        }
                    }Ok(false)
                }).await.ok().transpose()?.unwrap_or(false)
            };
            let result = json!({"state":if confirmed{"interrupted"}else{"interruption_requested"},"cleanup_confirmed":confirmed,"run_id":run});
            state.db.finish_chat_outgoing(key, &result)?;
            Ok(result)
        }
        "agent.message" => state.db.enqueue_agent_input(
            key,
            field(args, "agent_id")?,
            &rpc.subject,
            field(args, "text")?,
        ),
        _ => anyhow::bail!("unknown_channel_tool"),
    }
}

pub(crate) async fn ensure_runtime(state: &AppState, id: &str) -> Result<String> {
    if let Some(key) = id.strip_prefix("session:") {
        let binding = state.db.get_binding(key)?.context("agent_not_found")?;
        return crate::agent::ensure_binding_session(&state.agent, &state.db, &binding).await;
    }
    let _guard = state.entries.lock_local_task(&format!("agent:{id}")).await;
    let agent = state.db.allocate_channel_agent(id)?;
    let key = agent
        .session_key
        .as_deref()
        .context("agent_runtime_missing")?;
    let runtime = agent
        .session_id
        .as_deref()
        .context("agent_runtime_missing")?;
    if state.agent.service.contains(runtime) {
        return Ok(runtime.into());
    }
    let parts = key.split(':').collect::<Vec<_>>();
    ensure!(parts.len() == 3, "invalid_agent_runtime");
    let session = state.db.ensure_session(crate::db::EnsureSession {
        connection_id: "local_gui",
        platform: "local_gui",
        channel_id: parts[1],
        root_thread_ts: parts[2],
        channel_type: Some("agent_control"),
        initiator_user_id: None,
        initiator_message_ts: None,
    })?;
    let selection = crate::agent::SessionSelection {
        profile_id: agent.profile_id.clone(),
        model: agent.model.clone(),
        thinking: agent.thinking.clone(),
    };
    crate::agent::ensure_allocated_session(
        &state.agent,
        &state.db,
        &crate::db::SessionBindingRow::Normal(session),
        runtime,
        &selection,
        &crate::node::agent_prompt(&agent),
    )
    .await?;
    Ok(runtime.into())
}

pub(crate) async fn open_home(state: &AppState, id: &str) -> Result<Value> {
    let _guard = state
        .entries
        .lock_local_task(&format!("agent-home:{id}"))
        .await;
    let agent = state.db.node_agent(id)?.context("agent_not_found")?;
    let channel = if let Some(channel) = state.db.agent_home(id)? {
        channel
    } else {
        let key = format!("agent-home-create:{id}");
        let receipt = state.db.chat_begin(&key, id)?;
        let channel = if let Some(value) = receipt.result {
            serde_json::from_value(value)?
        } else {
            state
                .db
                .create_chat(&key, &receipt.object_id, &agent.name)?
        };
        let key = format!("agent-home-subscribe:{id}");
        let receipt = state.db.chat_begin(&key, &channel.chat_id)?;
        if receipt.result.is_none() {
            state.db.update_chat_preferences(
                &key,
                &channel.chat_id,
                id,
                &zork_client_types::chat::UpdatePreferences {
                    changes: zork_client_types::chat::PreferenceChanges {
                        subscribed: Some(true),
                        ..Default::default()
                    },
                    expected_revision: None,
                    start: None,
                },
            )?;
        }
        state.db.set_agent_home(id, &channel.chat_id)?;
        channel
    };
    state.db.record_chat_source(id, "local")?;
    // An empty home channel need not start or allocate a model context.
    Ok(json!({"agent":agent,"session_id":channel.chat_id,"chat_id":channel.chat_id}))
}

/// Shared configuration validation for tool and confirmed-message entry points.
pub(crate) async fn apply_configuration(
    state: &AppState,
    agent: &mut NodeAgent,
    fields: &Value,
) -> Result<()> {
    if let Some(name) = fields["name"].as_str() {
        agent.name = name.trim().into();
    }
    if let Some(avatar) = fields.get("avatar") {
        agent.avatar = serde_json::from_value(avatar.clone())?;
    }
    if let Some(instructions) = fields["instructions"].as_str() {
        agent.instructions = instructions.into();
    }
    if let Some(paths) = fields.get("skill_paths") {
        agent.skill_paths = serde_json::from_value(paths.clone())?;
    }
    if let Some(allowed) = fields.get("allowed_leaders") {
        agent.allowed_leaders = serde_json::from_value(allowed.clone())?;
    }
    if let Some(selection) = fields.get("selection") {
        let selection: crate::agent::SessionSelection = serde_json::from_value(selection.clone())?;
        let profiles = crate::agent::list_profiles(&state.agent).await?;
        let selection = crate::agent::resolve_selection(&profiles, &selection)
            .context("agent_selection_unavailable")?;
        agent.profile_id = selection.profile_id;
        agent.model = selection.model;
        agent.thinking = selection.thinking;
    }
    ensure!(
        !agent.name.is_empty()
            && agent.name.len() <= 160
            && !agent.name.chars().any(char::is_control),
        "invalid_agent_name"
    );
    ensure!(
        agent
            .avatar
            .as_deref()
            .is_none_or(crate::node::valid_avatar),
        "invalid_agent_avatar"
    );
    zork_config::validate_skill_paths(&agent.skill_paths)?;
    // Validate source resolution before accepting a configuration that all
    // of this Agent's existing execution contexts will consume.
    zork_config::load_config(&state.config.data_root)?
        .skills
        .sources(&state.config.data_root, &agent.skill_paths)?;
    Ok(())
}
