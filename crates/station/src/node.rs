//! Node administration. Provider credentials stay on this node and never enter
//! a Conversation, Task, Mesh envelope, or the public login-attempt response.
mod interactions;
mod resources;
use crate::state::AppState;
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;
use zork_profile::{DeviceCode, DeviceCodePoll};

#[derive(Clone)]
struct NodeState {
    app: AppState,
    attempts: Arc<Mutex<HashMap<String, Attempt>>>,
    http: reqwest::Client,
    local_token: String,
    catalog_ready: tokio::sync::watch::Sender<bool>,
    catalog_profiles: tokio::sync::watch::Sender<u64>,
}
struct Attempt {
    profile_id: String,
    pending: DeviceCode,
    expires: Instant,
    next_poll: Instant,
    completed: bool,
}
pub fn router(app: AppState) -> Router {
    let local_token = local_token(&app.config.data_root).expect("Gateway local control token");
    let state = NodeState {
        app,
        local_token,
        catalog_ready: tokio::sync::watch::channel(false).0,
        catalog_profiles: tokio::sync::watch::channel(0).0,
        attempts: Arc::new(Mutex::new(HashMap::new())),
        http: reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("provider HTTP client"),
    };
    tokio::spawn(deliver_pending(state.clone()));
    tokio::spawn(reconcile_catalog(state.clone()));
    Router::new()
        .route("/v1/node/status", get(node_status))
        .route("/v1/node/sync", post(sync_pull))
        .route("/v1/node/sync/commands", post(sync_mutate))
        .route("/v1/node/sync/receipt", post(sync_receipt))
        .route("/v1/node/info", get(node_info))
        .route("/v1/node/resources", get(node_resources))
        .route("/v1/node/resources/mcp/{id}", get(resources::mcp))
        .route("/v1/node/resources/service/{id}", get(resources::service))
        .route("/v1/node/pages", get(node_pages))
        .route(
            "/v1/node/chats/{chat_id}/messages/{message_id}/respond",
            post(interactions::respond),
        )
        .route("/v1/node/name", axum::routing::put(rename_node))
        .route("/v1/node/update", get(check_update).post(start_update))
        .route("/v1/node/conversations/read-markers", get(read_markers))
        .route(
            "/v1/node/agents/{id}/model",
            axum::routing::patch(update_model),
        )
        .route("/v1/node/mesh", get(mesh_config).put(save_mesh_config))
        .route(
            "/v1/node/mesh/invites",
            get(mesh_invites).post(create_mesh_invite),
        )
        .route(
            "/v1/node/mesh/invites/{id}",
            axum::routing::delete(revoke_mesh_invite),
        )
        .route("/v1/node/mesh/client-invites", post(create_client_invite))
        .route(
            "/v1/node/mesh/invites/{id}/approve",
            post(approve_client_invite),
        )
        .route("/v1/node/mesh/join", post(join_mesh))
        .route("/v1/node/mesh/members/remove", post(remove_mesh_member))
        .route("/v1/node/mesh/clients", post(register_mesh_client))
        .route("/v1/node/agents", get(agents).post(create_agent))
        .route("/v1/node/agents/{id}/open", post(open_agent))
        .route(
            "/v1/node/agents/{id}/skills",
            get(agent_skills).put(update_agent_skills),
        )
        .route(
            "/v1/node/agents/{id}/skills/catalog",
            get(resources::skills),
        )
        .route("/v1/node/agents/{id}/skills/{skill}", get(resources::skill))
        .route(
            "/v1/node/agents/{id}/avatar",
            axum::routing::put(update_avatar),
        )
        .route(
            "/v1/node/agents/{id}/grants",
            axum::routing::put(update_grants),
        )
        .route("/v1/node/agents/{id}/tasks", get(node_leader_tasks))
        .route("/v1/agent/workers", get(workers))
        .route("/v1/agent/tasks", get(leader_tasks).post(assign_task))
        .route("/v1/agent/tasks/{id}/rework", post(rework_task))
        .route("/v1/node/providers", get(providers))
        .route("/v1/node/profiles/{id}", get(get_profile).put(put_profile))
        .route("/v1/node/profiles/{id}/refresh", post(refresh_profile))
        .route(
            "/v1/node/profiles/{id}/models/refresh",
            post(refresh_profile_models),
        )
        .route(
            "/v1/node/profiles/{id}/models/enabled",
            axum::routing::put(set_profile_model_enabled),
        )
        .route(
            "/v1/node/profiles/{id}/name",
            axum::routing::put(rename_profile),
        )
        .route(
            "/v1/node/profiles/{id}/models",
            axum::routing::put(update_profile_models),
        )
        .route(
            "/v1/node/profiles/{id}/discovered-models",
            get(discover_models),
        )
        .route("/v1/node/auth", post(start_auth))
        .route("/v1/node/auth/{id}", post(poll_auth).delete(cancel_auth))
        .with_state(state)
}

pub(crate) fn local_token(root: &std::path::Path) -> anyhow::Result<String> {
    let path = root.join("run/node-token.json");
    match std::fs::read(&path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let token = zork_mesh::enrollment::secret();
            crate::enrollment::private_json(&path, &token)?;
            Ok(token)
        }
        Err(e) => Err(e.into()),
    }
}

/// Mesh admission checks membership before forwarding; direct callers require
/// the same local administrator token as the public product APIs.
async fn sync_pull(
    State(state): State<NodeState>,
    headers: HeaderMap,
    Json(request): Json<zork_client_types::sync::Pull>,
) -> Response {
    if !authorized(&state, &headers) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Node administrator token required",
        );
    }
    let owner = match sync_owner(&state) {
        Ok(owner) => owner,
        Err(_) => {
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "Node identity is unavailable",
            )
        }
    };
    let mut ready = state.catalog_ready.subscribe();
    if !matches!(
        tokio::time::timeout(Duration::from_secs(10), ready.wait_for(|ready| *ready)).await,
        Ok(Ok(_))
    ) {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Catalog initialization failed",
        );
    }
    let db = state.app.db.clone();
    match tokio::task::spawn_blocking(move || db.sync_pull(&owner, &request)).await {
        Ok(Ok(reply)) => Json(reply).into_response(),
        Ok(Err(_)) => error(
            StatusCode::BAD_REQUEST,
            "Invalid sync request or unavailable projection",
        ),
        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "Sync worker failed"),
    }
}

/// File-backed authorities are projected independently of readers. Subscribe
/// before the first read so changes during reconciliation cannot be lost.
async fn reconcile_catalog(state: NodeState) {
    let mut changes = state
        .app
        .db
        .realtime
        .listen(crate::realtime::PROFILES | crate::realtime::MESH | crate::realtime::NODE);
    let mut retry = zork_notify::retry::Retry::default();
    loop {
        changes.checkpoint();
        let before = state.app.db.realtime.current();
        let profiles = tokio::time::timeout(
            Duration::from_secs(5),
            crate::agent::list_profiles(&state.app.agent),
        )
        .await
        .ok()
        .and_then(|result| result.ok());
        let device = zork_config::load_config(&state.app.config.data_root).ok().zip(sync_owner(&state).ok()).map(|(config, owner)| {
            json!({"name":if config.mesh.name.trim().is_empty(){zork_config::device_name()}else{config.mesh.name},"origin":owner,"gateway":{"version":env!("CARGO_PKG_VERSION")},"update":update_info(&state)})
        });
        let projected_profiles = profiles.is_some();
        let complete = projected_profiles && device.is_some();
        let db = state.app.db.clone();
        let succeeded = match tokio::task::spawn_blocking(move || {
            db.sync_reconcile_catalog(device, profiles.as_deref())?;
            db.sync_maintain()
        })
        .await
        {
            Ok(Ok(())) => {
                state.catalog_ready.send_replace(true);
                if projected_profiles {
                    state.catalog_profiles.send_replace(before.profiles);
                }
                complete
            }
            result => {
                tracing::warn!(?result, "catalog reconciliation failed");
                false
            }
        };
        if succeeded {
            retry.reset();
            if changes.changed().await.is_err() {
                return;
            }
        } else {
            // Only failed reads are retried. A healthy file authority sleeps
            // until its kernel watcher or an explicit writer publishes a change.
            tokio::select! {
                _ = retry.wait() => {},
                result = changes.changed() => if result.is_err() { return; },
            }
        }
    }
}

async fn sync_mutate(
    State(state): State<NodeState>,
    headers: HeaderMap,
    Json(request): Json<zork_client_types::sync::Mutation>,
) -> Response {
    if !authorized(&state, &headers) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Node administrator token required",
        );
    }
    if let zork_client_types::sync::Action::AgentAvatar { avatar, .. } = &request.action {
        if !valid_avatar(avatar) {
            return error(StatusCode::BAD_REQUEST, "Unknown Agent avatar");
        }
    }
    let owner = match sync_owner(&state) {
        Ok(owner) => owner,
        Err(_) => return error(StatusCode::SERVICE_UNAVAILABLE, "Node identity unavailable"),
    };
    let _task_guard =
        if let zork_client_types::sync::Action::TaskDecision { id, .. } = &request.action {
            match state.app.db.sync_receipt(&owner, &request.request_id) {
                Ok(Some(_)) => None,
                Ok(None) => match crate::http::lock_task_decision(&state.app, id).await {
                    Ok(guard) => Some(guard),
                    Err(response) => return response,
                },
                Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "Receipt unavailable"),
            }
        } else {
            None
        };
    let db = state.app.db.clone();
    match tokio::task::spawn_blocking(move || db.sync_mutate(&owner, &request)).await {
        Ok(Ok(receipt)) => Json(receipt).into_response(),
        Ok(Err(_)) => error(StatusCode::CONFLICT, "Invalid or reused command identity"),
        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "Command worker failed"),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReceiptLookup {
    request_id: String,
}
async fn sync_receipt(
    State(state): State<NodeState>,
    headers: HeaderMap,
    Json(request): Json<ReceiptLookup>,
) -> Response {
    if !authorized(&state, &headers) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Node administrator token required",
        );
    }
    let result =
        sync_owner(&state).and_then(|owner| state.app.db.sync_receipt(&owner, &request.request_id));
    match result {
        Ok(receipt) => Json(receipt).into_response(),
        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "Receipt unavailable"),
    }
}

fn sync_owner(state: &NodeState) -> anyhow::Result<String> {
    match state.app.mesh.get() {
        Some(mesh) => Ok(mesh.origin().to_owned()),
        None => {
            anyhow::ensure!(
                !zork_config::load_config(&state.app.config.data_root)?
                    .mesh
                    .enabled,
                "Node identity is initializing"
            );
            state.app.db.sync_local_owner()
        }
    }
}

async fn node_status(State(state): State<NodeState>, headers: HeaderMap) -> Response {
    if !authorized(&state, &headers) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Node administrator token required",
        );
    }
    let root = match state.app.config.data_root.canonicalize() {
        Ok(root) => root,
        Err(_) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Node data directory is unavailable",
            );
        }
    };
    Json(json!({"protocol":1,"mesh_join":1,"pid":std::process::id(),"data_root":root,"origin":state.app.mesh.get().map(|m|m.origin())})).into_response()
}

async fn rename_node(
    State(state): State<NodeState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    if !authorized(&state, &headers) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Node administrator token required",
        );
    }
    let result: anyhow::Result<Value> = async {
        let name = zork_config::membership::validate_device_name(
            body["name"].as_str().unwrap_or_default(),
        )?;
        let config = zork_config::load_config(&state.app.config.data_root)?;
        if config.mesh.group.is_some() {
            let service = state
                .app
                .mesh
                .get()
                .ok_or_else(|| anyhow::anyhow!("mesh_not_ready"))?;
            service
                .enrollment
                .rename_device(&state.app, service.origin(), &name)
                .await
        } else {
            zork_config::update_config(&state.app.config.data_root, |config| {
                config.mesh.name = name.clone();
                Ok(())
            })?;
            if let Some(service) = state.app.mesh.get() {
                service.refresh(&state.app).await?;
            }
            Ok(json!({"name":name}))
        }
    }
    .await;
    mesh_result(result)
}

async fn node_pages(State(state): State<NodeState>, headers: HeaderMap) -> Response {
    if !authorized(&state, &headers) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Node administrator token required",
        );
    }
    match state.app.db.page_catalog() {
        Ok(catalog) => Json(catalog).into_response(),
        Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn node_resources(State(state): State<NodeState>, headers: HeaderMap) -> Response {
    if !authorized(&state, &headers) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Node administrator token required",
        );
    }
    let app = state.app.clone();
    match tokio::task::spawn_blocking(move || {
        use zork_client_types::resources::{ResourceCatalog, ResourceIssue, ResourceKind};
        let origin = crate::node_access::identity(&app);
        let mut catalog = ResourceCatalog {
            origin: origin.clone(),
            ..Default::default()
        };
        let services = app
            .mesh
            .get()
            .ok_or_else(|| anyhow::anyhow!("mesh_not_ready"))
            .and_then(|mesh| mesh.services.client_resources(&origin));
        for (kind, result) in [
            (
                ResourceKind::Skill,
                crate::node_tools::client_resources(&app),
            ),
            (ResourceKind::Mcp, crate::mcp::client_resources(&app)),
            (ResourceKind::Service, services),
        ] {
            match result {
                Ok(mut items) => catalog.items.append(&mut items),
                Err(error) => catalog.issues.push(ResourceIssue {
                    kind,
                    error: error.to_string(),
                }),
            }
        }
        catalog
    })
    .await
    {
        Ok(catalog) => Json(catalog).into_response(),
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Resource inventory unavailable",
        ),
    }
}

async fn node_info(State(state): State<NodeState>, headers: HeaderMap) -> Response {
    if !authorized(&state, &headers) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Node administrator token required",
        );
    }
    let release_version = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join("VERSION")))
        .and_then(|path| {
            use std::io::Read;
            let mut version = String::new();
            std::fs::File::open(path)
                .ok()?
                .take(65)
                .read_to_string(&mut version)
                .ok()?;
            Some(version)
        })
        .map(|version| version.trim().to_owned())
        .filter(|version| {
            !version.is_empty()
                && version.len() <= 64
                && version
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+'))
        });
    Json(json!({
        "name":zork_config::load_config(&state.app.config.data_root).ok().map(|c|c.mesh.name).filter(|n|!n.is_empty()).unwrap_or_else(zork_config::device_name),
        "gateway":{"version":env!("CARGO_PKG_VERSION"),"release_version":release_version,"running":true},
        "update":update_info(&state),
        "sync":{"protocol":zork_client_types::sync::PROTOCOL,"owner":sync_owner(&state).ok()}
    })).into_response()
}

fn update_info(state: &NodeState) -> Value {
    let eligibility = std::env::current_exe()
        .map_err(anyhow::Error::from)
        .and_then(|exe| zork_config::update::eligible(&state.app.config.data_root, &exe));
    json!({"supported":eligibility.is_ok(),"reason":eligibility.err().map(|e|e.to_string()),
        "status":zork_config::update::state(&state.app.config.data_root)})
}

async fn check_update(State(state): State<NodeState>, headers: HeaderMap) -> Response {
    if !authorized(&state, &headers) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Node administrator token required",
        );
    }
    let result: anyhow::Result<String> = async {
        let mut response = state
            .http
            .get(format!(
                "{}/latest/download/VERSION",
                zork_config::update::RELEASE_BASE
            ))
            .send()
            .await?
            .error_for_status()?;
        if response.content_length().is_some_and(|len| len > 128) {
            anyhow::bail!("Invalid release version response");
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            anyhow::ensure!(
                bytes.len() + chunk.len() <= 128,
                "Invalid release version response"
            );
            bytes.extend_from_slice(&chunk);
        }
        let version = String::from_utf8(bytes)?.trim().to_owned();
        anyhow::ensure!(
            zork_config::update::valid_version(&version),
            "Invalid release version response"
        );
        Ok(version)
    }
    .await;
    match result {
        Ok(version) => {
            Json(json!({"latest_version":version,"update":update_info(&state)})).into_response()
        }
        Err(e) => error(StatusCode::BAD_GATEWAY, &format!("检查更新失败：{e}")),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateRelease {
    version: String,
}

async fn start_update(
    State(state): State<NodeState>,
    headers: HeaderMap,
    Json(body): Json<UpdateRelease>,
) -> Response {
    if !authorized(&state, &headers) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Node administrator token required",
        );
    }
    if !zork_config::update::valid_version(&body.version) {
        return error(StatusCode::BAD_REQUEST, "Invalid release version");
    }
    let root = &state.app.config.data_root;
    let eligibility = std::env::current_exe()
        .map_err(anyhow::Error::from)
        .and_then(|exe| zork_config::update::eligible(root, &exe));
    if let Err(e) = eligibility {
        return error(StatusCode::CONFLICT, &e.to_string());
    }
    let result: anyhow::Result<()> = async {
        use std::process::Stdio;
        use tokio::io::{AsyncBufReadExt, BufReader};
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(root.join("logs/update.log"))?;
        let mut command = tokio::process::Command::new(root.join("bin/zork"));
        command
            .arg("upgrade")
            .arg("--data")
            .arg(root)
            .arg("--version")
            .arg(&body.version)
            .env_remove("ZORK_PARENT_PIPE")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(log));
        command.process_group(0);
        zork_config::service::prepare_child(command.as_std_mut());
        let mut child = command.spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("Update acknowledgement unavailable"))?;
        // The helper owns the lock and survives Gateway replacement. Reap it on
        // download failure while this Gateway is still alive.
        tokio::spawn(async move {
            let _ = child.wait().await;
        });
        let mut reply = String::new();
        tokio::time::timeout(
            Duration::from_secs(5),
            BufReader::new(stdout).read_line(&mut reply),
        )
        .await??;
        anyhow::ensure!(
            reply.trim() == "accepted",
            "升级未启动，可能已有升级正在进行；请刷新状态或查看 logs/update.log"
        );
        Ok(())
    }
    .await;
    match result {
        Ok(()) => (
            StatusCode::ACCEPTED,
            Json(json!({"update":update_info(&state)})),
        )
            .into_response(),
        Err(e) => error(StatusCode::CONFLICT, &e.to_string()),
    }
}

fn mesh_result(result: anyhow::Result<Value>) -> Response {
    match result {
        Ok(value) => Json(value).into_response(),
        Err(error_value) => {
            let message = error_value.to_string();
            let code = if message == "invite_expired" {
                StatusCode::GONE
            } else {
                StatusCode::CONFLICT
            };
            error(code, &message)
        }
    }
}
async fn create_mesh_invite(State(state): State<NodeState>, headers: HeaderMap) -> Response {
    if !authorized(&state, &headers) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Node administrator token required",
        );
    }
    let Some(service) = state.app.mesh.get() else {
        return error(StatusCode::CONFLICT, "mesh_not_ready");
    };
    mesh_result(service.enrollment.create(&state.app).await)
}
async fn create_client_invite(State(state): State<NodeState>, headers: HeaderMap) -> Response {
    if !authorized(&state, &headers) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Node administrator token required",
        );
    }
    let Some(service) = state.app.mesh.get() else {
        return error(StatusCode::CONFLICT, "mesh_not_ready");
    };
    mesh_result(
        service
            .enrollment
            .create_kind(&state.app, zork_mesh::enrollment::InviteKind::Client)
            .await,
    )
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ApproveClientInvite {
    origin: String,
    claim_id: String,
}
async fn approve_client_invite(
    State(state): State<NodeState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<ApproveClientInvite>,
) -> Response {
    if !authorized(&state, &headers) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Node administrator token required",
        );
    }
    let Some(service) = state.app.mesh.get() else {
        return error(StatusCode::CONFLICT, "mesh_not_ready");
    };
    mesh_result(
        service
            .enrollment
            .approve_client(&state.app, &id, &body.origin, &body.claim_id)
            .await,
    )
}
async fn mesh_invites(State(state): State<NodeState>, headers: HeaderMap) -> Response {
    if !authorized(&state, &headers) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Node administrator token required",
        );
    }
    let Some(service) = state.app.mesh.get() else {
        return Json(json!({"items":[]})).into_response();
    };
    mesh_result(service.enrollment.list(&state.app).await)
}
async fn revoke_mesh_invite(
    State(state): State<NodeState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !authorized(&state, &headers) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Node administrator token required",
        );
    }
    let Some(service) = state.app.mesh.get() else {
        return error(StatusCode::CONFLICT, "mesh_not_ready");
    };
    mesh_result(service.enrollment.revoke(&state.app, &id).await)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JoinMesh {
    invitation: String,
    name: Option<String>,
}
async fn join_mesh(
    State(state): State<NodeState>,
    headers: HeaderMap,
    Json(body): Json<JoinMesh>,
) -> Response {
    if !authorized(&state, &headers) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Node administrator token required",
        );
    }
    let Some(service) = state.app.mesh.get() else {
        return error(StatusCode::CONFLICT, "mesh_not_ready");
    };
    mesh_result(
        service
            .enrollment
            .join(&state.app, &body.invitation, body.name.as_deref())
            .await,
    )
}
async fn remove_mesh_member(
    State(state): State<NodeState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    if !authorized(&state, &headers) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Node administrator token required",
        );
    }
    let Some(service) = state.app.mesh.get() else {
        return error(StatusCode::CONFLICT, "mesh_not_ready");
    };
    let Some(origin) = body["origin"].as_str() else {
        return error(StatusCode::BAD_REQUEST, "device_identity_required");
    };
    mesh_result(service.enrollment.remove_member(&state.app, origin).await)
}
async fn register_mesh_client(
    State(state): State<NodeState>,
    headers: HeaderMap,
    Json(body): Json<zork_config::membership::MeshDevice>,
) -> Response {
    if !authorized(&state, &headers) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Node administrator token required",
        );
    }
    let Some(service) = state.app.mesh.get() else {
        return error(StatusCode::CONFLICT, "mesh_not_ready");
    };
    mesh_result(service.enrollment.register_client(&state.app, body).await)
}
async fn mesh_config(State(state): State<NodeState>, headers: HeaderMap) -> Response {
    if !authorized(&state, &headers) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Node administrator token required",
        );
    }
    match zork_config::load_config(&state.app.config.data_root) {
        Ok(config) => {
            Json(json!({"config":config.mesh,"origin":state.app.mesh.get().map(|m|m.origin())}))
                .into_response()
        }
        Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}
async fn save_mesh_config(
    State(state): State<NodeState>,
    headers: HeaderMap,
    Json(mesh): Json<zork_config::MeshConfig>,
) -> Response {
    if !authorized(&state, &headers) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Node administrator token required",
        );
    }
    let root = state.app.config.data_root.clone();
    let previous = match zork_config::load_config(&root) {
        Ok(c) => c.mesh,
        Err(e) => return error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    let restarting = !zork_mesh::managed::same_transport(&previous, &mesh);
    let result: anyhow::Result<()> = async {
        zork_mesh::managed::validate(&mesh)?;
        zork_config::update_config(&root, |config| {
            anyhow::ensure!(
                config.mesh.group == mesh.group,
                "mesh_membership_changed_refresh_required"
            );
            if let Some(group) = &config.mesh.group {
                for device in group
                    .members
                    .iter()
                    .filter(|m| state.app.mesh.get().is_some_and(|s| m.origin != s.origin()))
                {
                    anyhow::ensure!(
                        config.mesh.peers.iter().find(|p| p.origin == device.origin)
                            == mesh.peers.iter().find(|p| p.origin == device.origin),
                        "use_remove_device_for_mesh_members"
                    );
                }
            }
            config.mesh = mesh;
            Ok(())
        })?;
        if !restarting {
            if let Some(service) = state.app.mesh.get() {
                service.refresh(&state.app).await?;
            }
        }
        Ok(())
    }
    .await;
    if let Err(e) = result {
        return error(StatusCode::BAD_REQUEST, &e.to_string());
    }
    // Let the HTTP acknowledgement leave before the supervisor replaces us.
    if restarting {
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(250)).await;
            if let Ok(mut socket) =
                tokio::net::UnixStream::connect(zork_config::zork_sock_path(&root)).await
            {
                use tokio::io::AsyncWriteExt;
                let _ = socket.write_all(b"reload-mesh\n").await;
            }
        });
    }
    Json(json!({"saved":true,"restarting":restarting})).into_response()
}

fn authorized(state: &NodeState, headers: &HeaderMap) -> bool {
    if headers.get("authorization").and_then(|h| h.to_str().ok())
        == Some(format!("Bearer {}", state.local_token).as_str())
    {
        return true;
    }
    state
        .app
        .admin
        .admin_token
        .as_ref()
        .filter(|t| !t.is_empty())
        .is_some_and(|token| {
            headers.get("authorization").and_then(|h| h.to_str().ok())
                == Some(format!("Bearer {token}").as_str())
        })
}
fn error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({"error":message}))).into_response()
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 96
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
fn valid_profile_id(id: &str) -> bool {
    !id.is_empty()
        && id != "auto"
        && id.len() <= 96
        && !id.contains("..")
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}
async fn providers(State(state): State<NodeState>, headers: HeaderMap) -> Response {
    if !authorized(&state, &headers) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Node administrator token required",
        );
    }
    Json(zork_profile::list_providers()).into_response()
}
async fn put_profile(
    State(state): State<NodeState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    if !authorized(&state, &headers) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Node administrator token required",
        );
    }
    if !valid_profile_id(&id) {
        return error(StatusCode::BAD_REQUEST, "Invalid profile ID");
    }
    match crate::agent::put_profile(&state.app.agent, &id, &body).await {
        Ok(value) => Json(value).into_response(),
        Err(_) => error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Profile could not be saved; check its provider, model and credentials",
        ),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RenameProfile {
    name: String,
}

async fn rename_profile(
    State(state): State<NodeState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<RenameProfile>,
) -> Response {
    if !authorized(&state, &headers) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Node administrator token required",
        );
    }
    if !valid_profile_id(&id) {
        return error(StatusCode::BAD_REQUEST, "Invalid profile ID");
    }
    match state.app.agent.rename_profile(id, body.name).await {
        Ok(profile) => {
            state.app.db.realtime.notify(crate::realtime::PROFILES);
            Json(profile).into_response()
        }
        Err(e) => error(
            crate::agent::AgentError::from(e).status,
            "Profile name could not be updated",
        ),
    }
}

async fn refresh_profile(
    State(state): State<NodeState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !authorized(&state, &headers) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Node administrator token required",
        );
    }
    if !valid_profile_id(&id) {
        return error(StatusCode::BAD_REQUEST, "Invalid profile ID");
    }
    match state.app.agent.refresh_profile(id).await {
        Ok(profile) => {
            state.app.db.realtime.notify(crate::realtime::PROFILES);
            Json(profile).into_response()
        }
        Err(e) => error(
            crate::agent::AgentError::from(e).status,
            "Profile quota query failed",
        ),
    }
}

async fn get_profile(
    State(state): State<NodeState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !authorized(&state, &headers) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Node administrator token required",
        );
    }
    if !valid_profile_id(&id) {
        return error(StatusCode::BAD_REQUEST, "Invalid profile ID");
    }
    match crate::agent::get_profile(&state.app.agent, &id).await {
        Ok(value) => Json(value).into_response(),
        Err(e) => error(e.status, "Profile is unavailable"),
    }
}

/// Wait for the single file-authority coordinator, rather than starting a
/// competing projection writer. This marker is process-local, not a sync cursor.
async fn publish_model_catalog(state: &NodeState) {
    let mut applied = state.catalog_profiles.subscribe();
    let hints = state.app.db.realtime.subscribe();
    state.app.db.realtime.notify(crate::realtime::PROFILES);
    let requested = hints.borrow().profiles;
    let settled = async {
        loop {
            if *applied.borrow() >= requested {
                return;
            }
            if applied.changed().await.is_err() {
                return;
            }
        }
    };
    if tokio::time::timeout(Duration::from_secs(5), settled)
        .await
        .is_err()
    {
        tracing::warn!("Model catalog projection will retry in the background");
    }
}

async fn refresh_profile_models(
    State(state): State<NodeState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !authorized(&state, &headers) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Node administrator token required",
        );
    }
    if !valid_profile_id(&id) {
        return error(StatusCode::BAD_REQUEST, "Invalid profile ID");
    }
    match state.app.agent.refresh_profile_models(id).await {
        Ok(result) => {
            if result.added > 0 || result.configured > 0 {
                publish_model_catalog(&state).await;
            }
            Json(result).into_response()
        }
        Err(e) => error(
            crate::agent::AgentError::from(e).status,
            "Models could not be updated; check the connection or configure models manually",
        ),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelEnabled {
    model_id: String,
    enabled: bool,
}
async fn set_profile_model_enabled(
    State(state): State<NodeState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<ModelEnabled>,
) -> Response {
    if !authorized(&state, &headers) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Node administrator token required",
        );
    }
    if !valid_profile_id(&id) || body.model_id.is_empty() {
        return error(StatusCode::BAD_REQUEST, "Invalid model selection");
    }
    match state
        .app
        .agent
        .set_profile_model_enabled(id, body.model_id, body.enabled)
        .await
    {
        Ok((profile, changed)) => {
            if changed {
                publish_model_catalog(&state).await;
            }
            Json(profile).into_response()
        }
        Err(e) => error(
            crate::agent::AgentError::from(e).status,
            "Model state could not be saved; configure its token limits before enabling it",
        ),
    }
}

async fn discover_models(
    State(state): State<NodeState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !authorized(&state, &headers) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Node administrator token required",
        );
    }
    if !valid_profile_id(&id) {
        return error(StatusCode::BAD_REQUEST, "Invalid profile ID");
    }
    match crate::agent::discover_profile_models(&state.app.agent, &id).await {
        Ok(value) => Json(value).into_response(),
        Err(e) => error(
            e.status,
            "Could not retrieve models; check the connection or add a model manually",
        ),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateProfileModels {
    models: Vec<zork_profile::ProfileModel>,
    #[serde(default)]
    expected_models: Option<Vec<zork_profile::ProfileModel>>,
}

async fn update_profile_models(
    State(state): State<NodeState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<UpdateProfileModels>,
) -> Response {
    if !authorized(&state, &headers) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Node administrator token required",
        );
    }
    if !valid_profile_id(&id) || body.models.len() > 500 {
        return error(StatusCode::BAD_REQUEST, "Invalid profile model update");
    }
    match crate::agent::update_profile_models(
        &state.app.agent,
        &id,
        &body.models,
        body.expected_models.as_deref(),
    )
    .await
    {
        Ok(value) => {
            publish_model_catalog(&state).await;
            Json(value).into_response()
        }
        Err(e) => error(e.status, "Profile models could not be updated"),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StartAuth {
    profile_id: String,
    provider: String,
    billing: String,
}
async fn start_auth(
    State(state): State<NodeState>,
    headers: HeaderMap,
    Json(body): Json<StartAuth>,
) -> Response {
    if !authorized(&state, &headers) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Node administrator token required",
        );
    }
    if !valid_id(&body.profile_id) {
        return error(StatusCode::BAD_REQUEST, "Invalid profile ID");
    }
    let Ok(provider) = zork_profile::providers::get(&body.provider) else {
        return error(StatusCode::BAD_REQUEST, "Unknown provider");
    };
    if !provider.supports_device_code(&body.billing) {
        return error(
            StatusCode::BAD_REQUEST,
            "This provider and billing mode require an API key",
        );
    }
    let mut attempts = state.attempts.lock().await;
    attempts.retain(|_, a| a.expires > Instant::now());
    if attempts.len() >= 8 {
        return error(StatusCode::TOO_MANY_REQUESTS, "Too many pending sign-ins");
    }
    // A profile has one pending credential writer; a new attempt replaces it.
    attempts.retain(|_, a| a.profile_id != body.profile_id);
    let pending = match provider.start_device_code(&state.http).await {
        Ok(p) => p,
        Err(_) => {
            return error(
                StatusCode::BAD_GATEWAY,
                "Provider sign-in could not start; retry later",
            );
        }
    };
    if pending.billing != body.billing {
        return error(
            StatusCode::BAD_REQUEST,
            "Provider returned an unexpected billing mode",
        );
    }
    let ttl = chrono::DateTime::parse_from_rfc3339(&pending.expires_at)
        .ok()
        .and_then(|t| {
            (t.with_timezone(&chrono::Utc) - chrono::Utc::now())
                .to_std()
                .ok()
        })
        .unwrap_or(Duration::from_secs(300))
        .min(Duration::from_secs(1800));
    let id = ulid::Ulid::new().to_string();
    let value = json!({"id":id,"profile_id":body.profile_id,"verification_url":pending.verification_url,"user_code":pending.user_code,"interval_seconds":pending.interval_seconds,"expires_at":pending.expires_at,"flow":if pending.user_code.is_empty(){"browser_callback"}else{"device_code"}});
    attempts.insert(
        id,
        Attempt {
            profile_id: body.profile_id,
            pending,
            expires: Instant::now() + ttl,
            next_poll: Instant::now(),
            completed: false,
        },
    );
    Json(value).into_response()
}
#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct PollAuth {
    callback: Option<String>,
}
async fn poll_auth(
    State(state): State<NodeState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<PollAuth>,
) -> Response {
    if !authorized(&state, &headers) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Node administrator token required",
        );
    }
    let mut attempts = state.attempts.lock().await;
    let Some(attempt) = attempts.get_mut(&id) else {
        return error(StatusCode::NOT_FOUND, "Sign-in expired; start again");
    };
    if attempt.completed {
        return Json(json!({"status":"completed","profile_id":attempt.profile_id})).into_response();
    }
    if attempt.expires <= Instant::now() {
        attempts.remove(&id);
        return error(StatusCode::GONE, "Sign-in expired; start again");
    }
    if let Some(callback) = body.callback {
        if !matches!(
            attempt.pending.provider.as_str(),
            "anthropic" | "openrouter"
        ) || callback.len() > 8192
        {
            return error(StatusCode::BAD_REQUEST, "Unexpected browser callback");
        };
        attempt.pending.user_code = callback;
    }
    if attempt.next_poll > Instant::now() {
        return Json(json!({"status":"pending","retry_after_seconds":attempt.next_poll.saturating_duration_since(Instant::now()).as_secs().max(1)})).into_response();
    }
    let provider =
        zork_profile::providers::get(&attempt.pending.provider).expect("validated provider");
    match provider
        .poll_device_code(&state.http, &attempt.pending)
        .await
    {
        Ok(DeviceCodePoll::Pending {
            retry_after_seconds,
        }) => {
            let retry = retry_after_seconds.clamp(1, 60);
            attempt.next_poll = Instant::now() + Duration::from_secs(retry);
            Json(json!({"status":"pending","retry_after_seconds":retry})).into_response()
        }
        Ok(DeviceCodePoll::Completed { auth }) => {
            let Ok(template) = provider.template(&attempt.pending.billing) else {
                return error(StatusCode::BAD_REQUEST, "Provider template unavailable");
            };
            let mut document = template.document(auth);
            // Connecting an account is separate from configuring its models.
            // Keep an existing connection's explicit models during re-login.
            document["models"] =
                match crate::agent::get_profile(&state.app.agent, &attempt.profile_id).await {
                    Ok(existing)
                        if existing["provider"] == attempt.pending.provider
                            && existing["billing"] == attempt.pending.billing =>
                    {
                        existing["models"].clone()
                    }
                    _ => json!([]),
                };
            provider.decorate_document(&mut document);
            if crate::agent::put_profile(&state.app.agent, &attempt.profile_id, &document)
                .await
                .is_err()
            {
                return error(
                    StatusCode::BAD_GATEWAY,
                    "Could not save authorized profile; retry sign-in",
                );
            }
            attempt.completed = true;
            attempt.pending = DeviceCode::default();
            Json(json!({"status":"completed","profile_id":attempt.profile_id})).into_response()
        }
        Err(_) => {
            attempts.remove(&id);
            error(
                StatusCode::BAD_GATEWAY,
                "Authorization was rejected or expired; start again",
            )
        }
    }
}
async fn cancel_auth(
    State(state): State<NodeState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !authorized(&state, &headers) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Node administrator token required",
        );
    }
    state.attempts.lock().await.remove(&id);
    StatusCode::NO_CONTENT.into_response()
}

use crate::db::agents::{AgentRole, NodeAgent};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateAgent {
    id: String,
    name: String,
    #[serde(default)]
    avatar: Option<String>,
    role: AgentRole,
    #[serde(default)]
    profile_id: String,
    model: String,
    #[serde(alias = "effort")]
    thinking: String,
    #[serde(default)]
    instructions: String,
    #[serde(default)]
    skill_paths: Vec<std::path::PathBuf>,
    #[serde(default)]
    allowed_leaders: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentSkillPaths {
    paths: Vec<std::path::PathBuf>,
}

async fn update_agent_skills(
    State(state): State<NodeState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<AgentSkillPaths>,
) -> Response {
    if !authorized(&state, &headers) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Node administrator token required",
        );
    }
    if let Err(e) = zork_config::validate_skill_paths(&body.paths) {
        return error(StatusCode::BAD_REQUEST, &e.to_string());
    }
    if !matches!(state.app.db.node_agent(&id), Ok(Some(_))) {
        return error(StatusCode::NOT_FOUND, "Agent not found");
    }
    match state.app.db.update_agent_skill_paths(&id, body.paths) {
        Ok(agent) => Json(json!(agent)).into_response(),
        Err(e) => error(StatusCode::CONFLICT, &e.to_string()),
    }
}

async fn agent_skills(
    State(state): State<NodeState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !authorized(&state, &headers) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Node administrator token required",
        );
    }
    let Ok(Some(agent)) = state.app.db.node_agent(&id) else {
        return error(StatusCode::NOT_FOUND, "Agent not found");
    };
    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<serde_json::Value> {
        let root = &state.app.config.data_root;
        let sources = zork_config::load_config(root)?.skills.sources(root, &agent.skill_paths)?;
        Ok(json!({"paths": agent.skill_paths, "sources": sources, "catalog": zork_agent::skills::discover(&sources)}))
    }).await;
    match result {
        Ok(Ok(catalog)) => Json(catalog).into_response(),
        Ok(Err(e)) => error(StatusCode::BAD_REQUEST, &e.to_string()),
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Skill discovery interrupted",
        ),
    }
}

pub(crate) fn valid_avatar(avatar: &str) -> bool {
    matches!(
        avatar,
        "cat"
            | "bunny"
            | "bear"
            | "fox"
            | "panda"
            | "chick"
            | "dog"
            | "owl"
            | "koala"
            | "penguin"
            | "deer"
            | "octopus"
    )
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateAvatar {
    avatar: String,
}
async fn update_avatar(
    State(state): State<NodeState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<UpdateAvatar>,
) -> Response {
    if !authorized(&state, &headers) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Node administrator token required",
        );
    }
    if !valid_avatar(&body.avatar) {
        return error(StatusCode::BAD_REQUEST, "Unknown Agent avatar");
    }
    match state.app.db.node_agent(&id) {
        Ok(None) => return error(StatusCode::NOT_FOUND, "Agent not found"),
        Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "Could not read Agent"),
        Ok(Some(_)) => {}
    }
    match state.app.db.update_agent_avatar(&id, &body.avatar) {
        Ok(agent) => Json(json!(agent)).into_response(),
        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "Could not save avatar"),
    }
}
async fn read_markers(State(state): State<NodeState>, headers: HeaderMap) -> Response {
    if !authorized(&state, &headers) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Node administrator token required",
        );
    }
    match state.app.db.conversation_read_markers() {
        Ok(items) => Json(json!({"items":items})).into_response(),
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not read conversation markers",
        ),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateModel {
    #[serde(default)]
    profile_id: String,
    model: String,
    #[serde(alias = "effort")]
    thinking: Option<String>,
}
async fn update_model(
    State(state): State<NodeState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<UpdateModel>,
) -> Response {
    if !authorized(&state, &headers) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Node administrator token required",
        );
    }
    let _guard = state
        .app
        .entries
        .lock_local_task(&format!("agent:{id}"))
        .await;
    let Ok(Some(agent)) = state.app.db.node_agent(&id) else {
        return error(StatusCode::NOT_FOUND, "Agent not found");
    };
    let Ok(profiles) = crate::agent::list_profiles(&state.app.agent).await else {
        return error(StatusCode::BAD_GATEWAY, "Profiles unavailable");
    };
    let Some(model) = profiles
        .iter()
        .filter(|p| {
            (body.profile_id.is_empty()
                || body.profile_id == "auto"
                || p.profile_id == body.profile_id)
                && p.auth_configured
        })
        .flat_map(|p| p.models.iter())
        .find(|m| {
            m.enabled
                && m.id == body.model
                && body
                    .thinking
                    .as_ref()
                    .is_none_or(|t| m.thinking.contains(t))
        })
    else {
        return error(
            StatusCode::BAD_REQUEST,
            "Select a configured Profile and supported model",
        );
    };
    let requested = crate::agent::SessionSelection {
        profile_id: body.profile_id,
        model: body.model,
        thinking: body
            .thinking
            .unwrap_or_else(|| model.default_thinking.clone()),
    };
    let Some(selection) = crate::agent::resolve_selection(&profiles, &requested) else {
        return error(StatusCode::BAD_REQUEST, "Unsupported thinking level");
    };
    // Agent definitions are authoritative. Sessions pull this selection at
    // step boundaries, including dormant and recovered Worker sessions.
    let result = state.app.db.update_agent_model(
        &agent.id,
        &selection.profile_id,
        &selection.model,
        &selection.thinking,
    );
    match result {
        Ok(agent) => Json(json!(agent)).into_response(),
        Err(e) => error(StatusCode::BAD_GATEWAY, &e.to_string()),
    }
}

async fn agents(State(state): State<NodeState>, headers: HeaderMap) -> Response {
    if !authorized(&state, &headers) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Node administrator token required",
        );
    }
    match state.app.db.client_agents() {
        Ok(items) => Json(json!({"items":items})).into_response(),
        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "Could not read Agents"),
    }
}
async fn node_leader_tasks(
    State(state): State<NodeState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !authorized(&state, &headers) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Node administrator token required",
        );
    }
    match state.app.db.tasks_for_leader(&id) {
        Ok(items) => Json(json!({"items":items})).into_response(),
        Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateGrants {
    allowed_leaders: Vec<String>,
    expected_allowed_leaders: Vec<String>,
}
async fn update_grants(
    State(state): State<NodeState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<UpdateGrants>,
) -> Response {
    if !authorized(&state, &headers) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Node administrator token required",
        );
    }
    let result: anyhow::Result<NodeAgent> = async {
        let agents = state.app.db.node_agents()?;
        validate_grants(&state, &agents, &body.allowed_leaders)?;
        state.app.db.update_worker_grants(
            &id,
            &body.expected_allowed_leaders,
            &body.allowed_leaders,
        )
    }
    .await;
    match result {
        Ok(agent) => Json(json!(agent)).into_response(),
        Err(e) => error(StatusCode::CONFLICT, &e.to_string()),
    }
}

fn validate_grants(
    state: &NodeState,
    agents: &[NodeAgent],
    grants: &[String],
) -> anyhow::Result<()> {
    let config = zork_config::load_config(&state.app.config.data_root)?.mesh;
    anyhow::ensure!(grants.len() <= 32, "Too many Worker grants");
    for grant in grants {
        if let Some((origin, id)) = grant.split_once('/') {
            anyhow::ensure!(
                valid_id(id) && config.peers.iter().any(|p| p.origin == origin),
                "Remote Leader must belong to a paired node"
            );
        } else {
            anyhow::ensure!(
                agents
                    .iter()
                    .any(|a| a.id == *grant && a.role == AgentRole::Leader),
                "Worker grant references an unknown Leader"
            );
        }
    }
    Ok(())
}

async fn create_agent(
    State(state): State<NodeState>,
    headers: HeaderMap,
    Json(mut body): Json<CreateAgent>,
) -> Response {
    if !authorized(&state, &headers) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Node administrator token required",
        );
    }
    if body
        .avatar
        .as_deref()
        .is_some_and(|avatar| !valid_avatar(avatar))
        || !valid_id(&body.id)
        || body.name.trim().is_empty()
        || body.name.len() > 160
        || zork_config::validate_skill_paths(&body.skill_paths).is_err()
        || body.instructions.len() > 32000
        || body.allowed_leaders.len() > 32
    {
        return error(StatusCode::BAD_REQUEST, "Invalid Agent definition");
    }
    let selection = crate::agent::SessionSelection {
        profile_id: body.profile_id.clone(),
        model: body.model.clone(),
        thinking: body.thinking.clone(),
    };
    let Ok(profiles) = crate::agent::list_profiles(&state.app.agent).await else {
        return error(StatusCode::BAD_GATEWAY, "Profiles unavailable");
    };
    let Some(selection) = crate::agent::resolve_selection(&profiles, &selection) else {
        return error(
            StatusCode::BAD_REQUEST,
            "Select a configured Profile and supported model",
        );
    };
    body.profile_id = selection.profile_id;
    let _guard = state
        .app
        .entries
        .lock_local_task(&format!("agent:{}", body.id))
        .await;
    let result: anyhow::Result<NodeAgent> = async {
        if let Some(existing) = state.app.db.node_agent(&body.id)? {
            anyhow::ensure!(
                existing.name == body.name
                    && existing.avatar == body.avatar
                    && existing.role == body.role
                    && existing.profile_id == body.profile_id
                    && existing.model == body.model
                    && existing.thinking == body.thinking
                    && existing.skill_paths == body.skill_paths
                    && existing.instructions == body.instructions
                    && existing.allowed_leaders == body.allowed_leaders,
                "Agent ID is already in use"
            );
            return Ok(existing);
        }
        let agents = state.app.db.node_agents()?;
        anyhow::ensure!(agents.len() < 64, "This node supports up to 64 Agents");
        validate_grants(&state, &agents, &body.allowed_leaders)?;
        let conversation = format!("leader-{}", body.id);
        let leader = body.role == AgentRole::Leader;
        let agent = NodeAgent {
            id: body.id,
            name: body.name,
            avatar: body.avatar,
            role: body.role,
            profile_id: body.profile_id,
            model: body.model,
            thinking: body.thinking,
            instructions: body.instructions,
            skill_paths: body.skill_paths,
            allowed_leaders: body.allowed_leaders,
            session_key: leader.then(|| format!("local_gui:{conversation}:{conversation}")),
            session_id: leader.then(|| ulid::Ulid::new().to_string()),
        };
        state.app.db.insert_node_agent(&agent)?;
        Ok(agent)
    }
    .await;
    match result {
        Ok(agent) => Json(json!(agent)).into_response(),
        Err(e) => error(StatusCode::CONFLICT, &e.to_string()),
    }
}
pub(crate) fn agent_prompt(agent: &NodeAgent) -> String {
    format!(
        "{}\n\nAgent: {}\n{}",
        include_str!("../prompts/im-thread-base-instructions.md"),
        agent.name,
        agent.instructions
    )
}
async fn ensure_agent_session(
    state: &NodeState,
    agent: &NodeAgent,
    key: &str,
    runtime_id: &str,
) -> anyhow::Result<crate::db::SessionRow> {
    let parts = key.split(':').collect::<Vec<_>>();
    anyhow::ensure!(parts.len() == 3, "Invalid allocated session key");
    let session = state.app.db.ensure_session(crate::db::EnsureSession {
        connection_id: "local_gui",
        platform: "local_gui",
        channel_id: parts[1],
        root_thread_ts: parts[2],
        channel_type: Some(if agent.role == AgentRole::Leader {
            "leader_chat"
        } else {
            "worker_task"
        }),
        initiator_user_id: Some("local-user"),
        initiator_message_ts: None,
    })?;
    let binding = crate::db::SessionBindingRow::Normal(session.clone());
    let selection = crate::agent::SessionSelection {
        profile_id: agent.profile_id.clone(),
        model: agent.model.clone(),
        thinking: agent.thinking.clone(),
    };
    crate::agent::ensure_allocated_session(
        &state.app.agent,
        &state.app.db,
        &binding,
        runtime_id,
        &selection,
        &agent_prompt(agent),
    )
    .await?;
    state
        .app
        .status_projection
        .ensure(
            key,
            runtime_id,
            &session.connection_id,
            &session.channel_id,
            &session.root_thread_ts,
        )
        .await;
    state
        .app
        .db
        .get_session(key)?
        .ok_or_else(|| anyhow::anyhow!("Allocated session disappeared"))
}
async fn open_agent(
    State(state): State<NodeState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !authorized(&state, &headers) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Node administrator token required",
        );
    }
    match crate::channels::open_home(&state.app, &id).await {
        Ok(value) => Json(value).into_response(),
        Err(e) => error(StatusCode::BAD_GATEWAY, &e.to_string()),
    }
}
fn calling_leader(state: &NodeState, headers: &HeaderMap) -> Option<NodeAgent> {
    let key = headers.get("x-zork-session-key")?.to_str().ok()?;
    state
        .app
        .db
        .agent_for_session(key)
        .ok()
        .flatten()
        .filter(|a| a.role == AgentRole::Leader)
}

fn personal_mesh(state: &NodeState) -> bool {
    state.app.mesh.get().is_some_and(|service| {
        zork_config::load_config(&state.app.config.data_root)
            .ok()
            .and_then(|c| c.mesh.group)
            .is_some_and(|group| group.contains(service.origin()))
    })
}
async fn workers(State(state): State<NodeState>, headers: HeaderMap) -> Response {
    let Some(leader) = calling_leader(&state, &headers) else {
        return error(
            StatusCode::FORBIDDEN,
            "Only a Leader session can discover Workers",
        );
    };
    match state.app.db.node_agents() {
        Ok(agents) => {
            let mut items = agents
                .into_iter()
                .filter(|a| {
                    a.role == AgentRole::Worker
                        && (personal_mesh(&state) || a.allowed_leaders.contains(&leader.id))
                })
                .map(|a| json!({"id":a.id,"name":a.name,"node":"local"}))
                .collect::<Vec<_>>();
            if let Some(mesh) = state.app.mesh.get() {
                items.extend(mesh.remote_workers(&leader.id).await);
            }
            Json(json!({"items":items})).into_response()
        }
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Worker catalog unavailable",
        ),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AssignTask {
    request_id: String,
    worker_id: String,
    goal: String,
    #[serde(default)]
    attachment_ids: Vec<String>,
}
async fn assign_task(
    State(state): State<NodeState>,
    headers: HeaderMap,
    Json(mut body): Json<AssignTask>,
) -> Response {
    let Some(leader) = calling_leader(&state, &headers) else {
        return error(
            StatusCode::FORBIDDEN,
            "Only a Leader session can assign tasks",
        );
    };
    if !valid_id(&body.request_id) || body.goal.trim().is_empty() || body.goal.len() > 64000 {
        return error(
            StatusCode::BAD_REQUEST,
            "A stable request_id and bounded goal are required",
        );
    }
    if body.attachment_ids.len() > zork_client_types::files::MAX_FILES {
        return error(StatusCode::BAD_REQUEST, "too_many_attachments");
    }
    if !body.attachment_ids.is_empty() {
        let references = body
            .attachment_ids
            .iter()
            .map(|id| {
                state
                    .app
                    .db
                    .conversation_file_ref(leader.session_key.as_deref().unwrap_or_default(), id)
            })
            .collect::<anyhow::Result<Vec<_>>>();
        match references {
            Ok(files) if zork_client_types::files::valid(&files) => {
                body.goal = zork_client_types::files::compose(&body.goal, &files)
            }
            Ok(_) => return error(StatusCode::BAD_REQUEST, "invalid_attachments"),
            Err(e) => return error(StatusCode::BAD_REQUEST, &e.to_string()),
        }
    }
    if body.worker_id.contains('/') {
        if let Some(mesh) = state.app.mesh.get() {
            if mesh
                .remote_worker_available(&leader.id, &body.worker_id)
                .await
                == Some(false)
            {
                return error(
                    StatusCode::FORBIDDEN,
                    "Remote Worker is not granted to this Leader",
                );
            }
        }
        let _guard = state
            .app
            .entries
            .lock_local_task(&format!("assignment:{}:{}", leader.id, body.request_id))
            .await;
        return match state
            .app
            .mesh
            .get()
            .ok_or_else(|| anyhow::anyhow!("Mesh is disabled"))
            .and_then(|mesh| {
                mesh.assign_worker(
                    &state.app,
                    &leader,
                    &body.worker_id,
                    &body.request_id,
                    &body.goal,
                )
            }) {
            Ok(value) => Json(value).into_response(),
            Err(e) => error(StatusCode::CONFLICT, &e.to_string()),
        };
    }
    let Ok(Some(worker)) = state.app.db.node_agent(&body.worker_id) else {
        return error(StatusCode::NOT_FOUND, "Worker not found");
    };
    if worker.role != AgentRole::Worker
        || !(personal_mesh(&state) || worker.allowed_leaders.contains(&leader.id))
    {
        return error(
            StatusCode::FORBIDDEN,
            "Worker is not granted to this Leader",
        );
    }
    let _guard = state
        .app
        .entries
        .lock_local_task(&format!("assignment:{}:{}", leader.id, body.request_id))
        .await;
    let result:anyhow::Result<Value>=async {
        let (key,runtime_id,status)=state.app.db.worker_task_allocation(&leader.id,&body.request_id,&worker.id,&body.goal)?;
        if status!="sent" {
            let session=ensure_agent_session(&state,&worker,&key,&runtime_id).await?;
            state.app.db.ensure_product_task(&key)?;
            let goal=state.app.db.copy_message_files(leader.session_key.as_deref().ok_or_else(||anyhow::anyhow!("leader session missing"))?,&session,&body.goal)?;
            let input=crate::http::conversation_files::agent_content(&state.app,&session.key,&goal).await?;
            state.app.entries.accept_local_user_message(&session,&format!("assignment-{}-{}",leader.id,body.request_id),&goal)?;
            state.app.db.set_worker_task_state(&key,"sending")?;
            crate::agent::append_mailbox_id(&state.app.agent,&runtime_id,&format!("assignment-{}",body.request_id),&input).await?;
            state.app.db.set_worker_task_state(&key,"sent")?;
        }
        Ok(json!({"task":state.app.db.product_task_for_session(&key)?,"worker_id":worker.id,"session_id":runtime_id}))
    }.await;
    match result {
        Ok(value) => Json(value).into_response(),
        Err(e) => error(StatusCode::CONFLICT, &e.to_string()),
    }
}

async fn leader_tasks(State(state): State<NodeState>, headers: HeaderMap) -> Response {
    let Some(leader) = calling_leader(&state, &headers) else {
        return error(
            StatusCode::FORBIDDEN,
            "Only a Leader can list assigned tasks",
        );
    };
    match state.app.db.tasks_for_leader(&leader.id) {
        Ok(tasks) => {
            let result: anyhow::Result<Vec<Value>> = tasks
                .into_iter()
                .map(|task| {
                    let artifacts = state.app.db.list_artifacts(Some(&task.task_id))?;
                    let mut value = serde_json::to_value(task)?;
                    value["artifacts"] = json!(artifacts);
                    Ok(value)
                })
                .collect();
            match result {
                Ok(items) => Json(json!({"items":items})).into_response(),
                Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
            }
        }
        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "Tasks unavailable"),
    }
}

async fn deliver_pending(state: NodeState) {
    let mut changes = state
        .app
        .db
        .realtime
        .listen(crate::realtime::WORK | crate::realtime::MESH);
    let mut retry = zork_notify::retry::Retry::new(Duration::from_secs(2), Duration::from_secs(2));
    loop {
        changes.checkpoint();
        let mut failed = false;
        if let Ok(pending) = state.app.db.pending_worker_tasks() {
            for (leader_id, request_id, worker_id, goal) in pending {
                let Ok(Some(leader)) = state.app.db.node_agent(&leader_id) else {
                    continue;
                };
                let mut headers = HeaderMap::new();
                if let Some(key) = leader.session_key {
                    if let Ok(value) = key.parse() {
                        headers.insert("x-zork-session-key", value);
                        let response = assign_task(
                            State(state.clone()),
                            headers,
                            Json(AssignTask {
                                request_id,
                                worker_id,
                                goal,
                                attachment_ids: vec![],
                            }),
                        )
                        .await;
                        failed |= response.status().is_server_error();
                    }
                }
            }
        }
        if let Ok(notifications) = state.app.db.pending_leader_notifications() {
            for (id, leader_id, content) in notifications {
                let _agent_guard = state
                    .app
                    .entries
                    .lock_local_task(&format!("agent:{leader_id}"))
                    .await;
                let Ok(Some(leader)) = state.app.db.node_agent(&leader_id) else {
                    continue;
                };
                let (Some(key), Some(session)) =
                    (leader.session_key.as_deref(), leader.session_id.as_deref())
                else {
                    continue;
                };
                let _guard = state.app.entries.lock_local_task(session).await;
                if ensure_agent_session(&state, &leader, key, session)
                    .await
                    .is_ok()
                    && crate::agent::append_mailbox_id(
                        &state.app.agent,
                        session,
                        &format!("worker-result-{id}"),
                        &content,
                    )
                    .await
                    .is_ok()
                {
                    let _ = state.app.db.finish_leader_notification(&id);
                } else {
                    failed = true;
                }
            }
        }
        if failed {
            retry.wait().await;
        } else if changes.changed().await.is_err() {
            break;
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReworkTask {
    request_id: String,
    goal: String,
    expected_revision: i64,
}
async fn rework_task(
    State(state): State<NodeState>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
    Json(body): Json<ReworkTask>,
) -> Response {
    let Some(leader) = calling_leader(&state, &headers) else {
        return error(StatusCode::FORBIDDEN, "Only a Leader can request rework");
    };
    if !valid_id(&body.request_id) || body.goal.trim().is_empty() || body.goal.len() > 64000 {
        return error(
            StatusCode::BAD_REQUEST,
            "A request_id and bounded rework goal are required",
        );
    }
    let result: anyhow::Result<Value> = async {
        let (owner, worker_id, key) = state
            .app
            .db
            .worker_task_owner(&task_id)?
            .ok_or_else(|| anyhow::anyhow!("Worker task not found"))?;
        anyhow::ensure!(owner == leader.id, "Task belongs to another Leader");
        if worker_id.contains('/') {
            anyhow::ensure!(
                body.goal.len() <= 32 * 1024,
                "Remote rework goal is too large"
            );
            state.app.db.mesh_queue_rework(
                &task_id,
                &body.request_id,
                &body.goal,
                body.expected_revision,
            )?;
            return Ok(json!({"task":state.app.db.product_task(&task_id)?,"queued":true}));
        }
        let worker = state
            .app
            .db
            .node_agent(&worker_id)?
            .ok_or_else(|| anyhow::anyhow!("Worker missing"))?;
        anyhow::ensure!(
            personal_mesh(&state) || worker.allowed_leaders.contains(&leader.id),
            "Worker permission was revoked"
        );
        let session = state
            .app
            .db
            .get_session(&key)?
            .ok_or_else(|| anyhow::anyhow!("Task session missing"))?;
        let id = session
            .id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Task session unavailable"))?;
        let _guard = state.app.entries.lock_local_task(id).await;
        let receipt = format!("rework-{task_id}-{}", body.request_id);
        if !state
            .app
            .db
            .has_message_receipt(&receipt, &key, &body.goal)?
        {
            let task = state
                .app
                .db
                .product_task(&task_id)?
                .ok_or_else(|| anyhow::anyhow!("Task missing"))?;
            anyhow::ensure!(
                !task.state.is_closed(),
                "The user must reopen this closed task before rework"
            );
            anyhow::ensure!(
                task.revision == body.expected_revision,
                "Task revision changed; inspect it again"
            );
            state
                .app
                .entries
                .accept_local_user_message(&session, &receipt, &body.goal)?;
        }
        crate::agent::append_mailbox_id(&state.app.agent, id, &receipt, &body.goal).await?;
        Ok(json!({"task":state.app.db.product_task(&task_id)?,"session_id":id}))
    }
    .await;
    match result {
        Ok(value) => Json(value).into_response(),
        Err(e) => error(StatusCode::CONFLICT, &e.to_string()),
    }
}
