use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, Request, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::error;

use crate::agent;
use crate::control_github;
use crate::http as runtime_http;
use crate::state::AppState as RuntimeState;

pub fn router(state: RuntimeState) -> Router {
    Router::new()
        .route("/readyz", get(admin_readyz))
        .route("/healthz", get(admin_readyz))
        .route("/", get(|| async { Redirect::temporary("/admin") }))
        .route("/admin/api/overview", get(overview))
        .route("/admin/api/status", get(status))
        .route("/admin/api/settings", get(get_settings).put(put_settings))
        .route("/admin/api/operations", get(operations))
        .route("/admin/api/audit", get(audit))
        .route("/admin/api/profiles", get(list_profiles))
        .route(
            "/admin/api/profiles/{profile_id}",
            axum::routing::put(put_profile).delete(delete_profile),
        )
        .route(
            "/admin/api/github-authors",
            get(list_authors).post(upsert_author),
        )
        .route("/admin/api/github-authors/{id}", delete(delete_author))
        .route("/admin/api/reload", post(trigger_reload))
        .route("/admin/api/deploy", post(not_configured))
        .route("/admin/api/rollback", post(not_configured))
        .route("/admin/api/sessions", get(runtime_http::list_sessions))
        .route("/admin/api/events", get(runtime_http::events))
        .route("/admin/api/logs", get(runtime_http::logs))
        .route("/admin/api/preflight", get(runtime_http::preflight))
        .route(
            "/admin/api/sessions/{session_key}/timeline",
            get(runtime_http::timeline),
        )
        .route(
            "/admin/api/sessions/{session_key}/timeline-events/{event_id}",
            get(runtime_http::timeline_event),
        )
        .route(
            "/admin/api/sessions/{session_key}/reset",
            post(runtime_http::reset_session),
        )
        .route(
            "/admin/api/sessions/{session_key}/selection",
            axum::routing::put(update_session_selection),
        )
        .route(
            "/admin/api/sessions/{session_key}/jobs/{job_id}/cancel",
            post(runtime_http::cancel_job),
        )
        .route(
            "/admin/api/sessions/{session_key}",
            delete(runtime_http::delete_session),
        )
        .fallback(fallback)
        .with_state(state)
}

async fn admin_readyz() -> impl IntoResponse {
    Json(json!({ "ok": true, "service": "zork-gateway", "pid": std::process::id() }))
}

async fn overview(State(state): State<RuntimeState>) -> Response {
    merge_page(state, true).await
}

async fn status(State(state): State<RuntimeState>) -> Response {
    merge_page(state, false).await
}

async fn merge_page(state: RuntimeState, include_sessions: bool) -> Response {
    let snapshot = state.db.snapshot().unwrap_or_else(|error| {
        error!(error = %error, "runtime snapshot failed");
        json!({ "ok": false, "error": error.to_string() })
    });
    let operations = state.admin.db.list_operations(10).unwrap_or_default();
    let audit = state.admin.db.list_audit(None, 10).unwrap_or_default();
    let profiles = profiles(&state).await;
    let github_author_mappings = control_github::list_mappings(&state.config).unwrap_or_default();
    let realtime = snapshot.get("realtime").cloned().unwrap_or(json!({}));
    let mut state_value = snapshot.get("state").cloned().unwrap_or(json!({}));
    if !include_sessions {
        if let Value::Object(map) = &mut state_value {
            map.remove("sessions");
        }
    }
    Json(json!({
        "ok": true,
        "service": {
            "name": "zork-gateway",
            "mode": "single",
            "startedAt": state.admin.started_at,
            "runtimeBaseUrl": format!("http://127.0.0.1:{}", state.config.bind_addr.port()),
            "adminTokenConfigured": state.admin.admin_token.is_some(),
            "dataRoot": state.config.data_root,
            "slackConfigured": zork_config::load_config(&state.config.data_root)
                .ok()
                .is_some_and(|file| zork_config::slack_configured(&file)),
        },
        "profiles": profiles,
        "githubAuthorMappings": github_author_mappings,
        "githubAccounts": [],
        "githubPrIdentities": [],
        "deployment": { "ok": false, "error": "not_configured" },
        "realtime": realtime,
        "operations": operations,
        "auditEvents": audit,
        "state": state_value,
        "platforms": {
            "slack": { "state": "ready", "enabled": true }
        },
    }))
    .into_response()
}

async fn profiles(state: &RuntimeState) -> Value {
    agent::profiles_value(&state.config)
        .await
        .unwrap_or_else(|error| json!({ "ok": false, "error": error.to_string() }))
}

#[derive(Deserialize, Default)]
struct SettingsBody {
    slack: Option<SlackSettingsBody>,
}

#[derive(Deserialize, Default)]
struct SlackSettingsBody {
    #[serde(rename = "appToken")]
    app_token: Option<String>,
    #[serde(rename = "botToken")]
    bot_token: Option<String>,
}

async fn get_settings(State(state): State<RuntimeState>, headers: HeaderMap) -> Response {
    if !authorize(&headers, &state) {
        return unauthorized();
    }
    let file = zork_config::load_config(&state.config.data_root)
        .unwrap_or_else(|_| zork_config::FileConfig::default());
    let app_set = !file.slack.app_token.trim().is_empty();
    let bot_set = !file.slack.bot_token.trim().is_empty();
    Json(json!({
        "ok": true,
        "slack": {
            "configured": app_set && bot_set,
            "appTokenSet": app_set,
            "botTokenSet": bot_set,
            "appTokenConfigured": app_set,
            "botTokenConfigured": bot_set,
        },
    }))
    .into_response()
}

async fn put_settings(
    State(state): State<RuntimeState>,
    headers: HeaderMap,
    body: Option<Json<SettingsBody>>,
) -> Response {
    if !authorize(&headers, &state) {
        return unauthorized();
    }
    let Some(Json(body)) = body else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": "invalid_body" })),
        )
            .into_response();
    };
    let mut file = match zork_config::load_config(&state.config.data_root) {
        Ok(file) => file,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "ok": false, "error": error.to_string() })),
            )
                .into_response()
        }
    };
    if let Some(slack) = body.slack {
        if let Some(app_token) = slack.app_token {
            file.slack.app_token = app_token.trim().to_string();
        }
        if let Some(bot_token) = slack.bot_token {
            file.slack.bot_token = bot_token.trim().to_string();
        }
    }
    if let Err(error) = zork_config::save_config(&state.config.data_root, &file) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": error.to_string() })),
        )
            .into_response();
    }
    let app_set = !file.slack.app_token.trim().is_empty();
    let bot_set = !file.slack.bot_token.trim().is_empty();
    Json(json!({
        "ok": true,
        "slack": {
            "configured": app_set && bot_set,
            "appTokenSet": app_set,
            "botTokenSet": bot_set,
        },
    }))
    .into_response()
}

async fn operations(State(state): State<RuntimeState>, headers: HeaderMap) -> Response {
    if !authorize(&headers, &state) {
        return unauthorized();
    }
    let items = state.admin.db.list_operations(100).unwrap_or_default();
    Json(json!({ "ok": true, "operations": items })).into_response()
}

async fn audit(State(state): State<RuntimeState>, headers: HeaderMap) -> Response {
    if !authorize(&headers, &state) {
        return unauthorized();
    }
    let items = state.admin.db.list_audit(None, 200).unwrap_or_default();
    Json(json!({ "ok": true, "events": items })).into_response()
}

async fn list_profiles(State(state): State<RuntimeState>, headers: HeaderMap) -> Response {
    if !authorize(&headers, &state) {
        return unauthorized();
    }
    match agent::profiles_value(&state.config).await {
        Ok(value) => Json(value).into_response(),
        Err(error) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "ok": false, "error": error.to_string() })),
        )
            .into_response(),
    }
}

async fn put_profile(
    State(state): State<RuntimeState>,
    headers: HeaderMap,
    Path(profile_id): Path<String>,
    body: Option<Json<Value>>,
) -> Response {
    if !authorize(&headers, &state) {
        return unauthorized();
    }
    let Some(Json(body)) = body else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": "invalid_body" })),
        )
            .into_response();
    };
    let operation_input = redact_operation_input(&body);
    match agent::put_profile(&state.config, &profile_id, &body).await {
        Ok(value) => {
            let _ =
                state
                    .admin
                    .db
                    .record_operation("profile.put", &operation_input, Ok(value.clone()));
            Json(value).into_response()
        }
        Err(error) => {
            let _ = state.admin.db.record_operation(
                "profile.put",
                &operation_input,
                Err(error.to_string()),
            );
            (
                error.status,
                Json(json!({ "ok": false, "error": error.message })),
            )
                .into_response()
        }
    }
}

async fn delete_profile(
    State(state): State<RuntimeState>,
    headers: HeaderMap,
    Path(profile_id): Path<String>,
) -> Response {
    if !authorize(&headers, &state) {
        return unauthorized();
    }
    match agent::delete_profile(&state.config, &profile_id).await {
        Ok(()) => {
            let _ = state.admin.db.record_operation(
                "profile.delete",
                &json!({ "profile_id": profile_id }),
                Ok(json!({ "ok": true })),
            );
            StatusCode::NO_CONTENT.into_response()
        }
        Err(error) => {
            let _ = state.admin.db.record_operation(
                "profile.delete",
                &json!({ "profile_id": profile_id }),
                Err(error.to_string()),
            );
            (
                error.status,
                Json(json!({ "ok": false, "error": error.message })),
            )
                .into_response()
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionSelectionBody {
    profile_id: String,
    model: String,
    thinking: String,
}

async fn update_session_selection(
    State(state): State<RuntimeState>,
    headers: HeaderMap,
    Path(session_key): Path<String>,
    body: Result<Json<SessionSelectionBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    if !authorize(&headers, &state) {
        return unauthorized();
    }
    let Json(body) = match body {
        Ok(body) => body,
        Err(_) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(json!({ "ok": false, "error": "invalid_selection" })),
            )
                .into_response()
        }
    };
    if body.profile_id.trim().is_empty()
        || body.model.trim().is_empty()
        || body.thinking.trim().is_empty()
    {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "ok": false, "error": "invalid_selection" })),
        )
            .into_response();
    }
    let session = match state.db.get_session(&session_key) {
        Ok(Some(session)) => session,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "ok": false, "error": "session_not_found" })),
            )
                .into_response()
        }
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "ok": false, "error": error.to_string() })),
            )
                .into_response()
        }
    };
    let profiles = match agent::list_profiles(&state.config).await {
        Ok(profiles) => profiles,
        Err(error) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "ok": false, "error": error.to_string() })),
            )
                .into_response()
        }
    };
    let requested = agent::SessionSelection {
        profile_id: body.profile_id,
        model: body.model,
        thinking: body.thinking,
    };
    let Some(selection) = agent::resolve_selection(&profiles, &requested) else {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "ok": false, "error": "selection_unavailable" })),
        )
            .into_response();
    };
    let (session_id, workspace_path) = match session.id.as_deref() {
        Some(session_id) => {
            match agent::update_selection(&state.config, session_id, &selection).await {
                Ok(_) => (session_id.to_owned(), session.workspace_path.clone()),
                Err(error) => {
                    return (
                        error.status,
                        Json(json!({ "ok": false, "error": error.message })),
                    )
                        .into_response()
                }
            }
        }
        None => match agent::create_session(
            &state.config,
            &selection,
            Some(include_str!("../prompts/slack-thread-base-instructions.md")),
            &session.workspace_path,
        )
        .await
        {
            Ok(created) => (created.session_id, created.workspace),
            Err(error) => {
                return (
                    error.status,
                    Json(json!({ "ok": false, "error": error.message })),
                )
                    .into_response()
            }
        },
    };
    if let Err(error) = state.db.set_agent_session(
        &session.key,
        &session_id,
        &workspace_path,
        &selection.profile_id,
        &selection.model,
        &selection.thinking,
    ) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": error.to_string() })),
        )
            .into_response();
    }
    let session = state
        .db
        .get_session(&session.key)
        .ok()
        .flatten()
        .and_then(|session| state.db.session_summary(&session).ok());
    Json(json!({
        "ok": true,
        "selection": selection,
        "session": session,
    }))
    .into_response()
}

async fn list_authors(State(state): State<RuntimeState>, headers: HeaderMap) -> Response {
    if !authorize(&headers, &state) {
        return unauthorized();
    }
    match control_github::list_mappings(&state.config) {
        Ok(items) => Json(json!({ "ok": true, "authors": items })).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": error.to_string() })),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct UpsertAuthorBody {
    #[serde(rename = "slackUserId")]
    slack_user_id: Option<String>,
    #[serde(rename = "githubLogin")]
    github_login: Option<String>,
    #[serde(rename = "githubToken")]
    github_token: Option<String>,
    #[serde(rename = "slackUserName")]
    slack_user_name: Option<String>,
    #[serde(rename = "githubUserName")]
    github_user_name: Option<String>,
}

async fn upsert_author(
    State(state): State<RuntimeState>,
    headers: HeaderMap,
    body: Option<Json<UpsertAuthorBody>>,
) -> Response {
    if !authorize(&headers, &state) {
        return unauthorized();
    }
    let Some(Json(body)) = body else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": "invalid_body" })),
        )
            .into_response();
    };
    let Some(slack_user_id) = body.slack_user_id.filter(|value| !value.trim().is_empty()) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": "missing_slack_user_id" })),
        )
            .into_response();
    };
    if body.github_login.is_none() && body.github_token.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": "missing_github_identity" })),
        )
            .into_response();
    }
    let payload = json!({
        "slackUserId": slack_user_id,
        "slackUserName": body.slack_user_name,
        "githubLogin": body.github_login,
        "githubUserName": body.github_user_name,
        "githubToken": body.github_token,
    });
    let operation_input = redact_operation_input(&payload);
    match control_github::upsert_mapping(
        &state.config,
        &slack_user_id,
        payload
            .get("githubLogin")
            .and_then(Value::as_str)
            .unwrap_or(""),
    ) {
        Ok(value) => {
            let _ = state.admin.db.record_operation(
                "github_author.upsert",
                &operation_input,
                Ok(value.clone()),
            );
            Json(json!({ "ok": true, "author": value })).into_response()
        }
        Err(error) => {
            let _ = state.admin.db.record_operation(
                "github_author.upsert",
                &operation_input,
                Err(error.to_string()),
            );
            (
                StatusCode::BAD_REQUEST,
                Json(json!({ "ok": false, "error": error.to_string() })),
            )
                .into_response()
        }
    }
}

async fn delete_author(
    State(state): State<RuntimeState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !authorize(&headers, &state) {
        return unauthorized();
    }
    match control_github::delete_mapping(&state.config, &id) {
        Ok(()) => {
            let _ = state.admin.db.record_operation(
                "github_author.delete",
                &json!({ "id": id }),
                Ok(json!({ "ok": true })),
            );
            Json(json!({ "ok": true })).into_response()
        }
        Err(error) => {
            let _ = state.admin.db.record_operation(
                "github_author.delete",
                &json!({ "id": id }),
                Err(error.to_string()),
            );
            (
                StatusCode::BAD_REQUEST,
                Json(json!({ "ok": false, "error": error.to_string() })),
            )
                .into_response()
        }
    }
}

/// Asks the supervisor to drain, stop, and restart every supervised process.
async fn trigger_reload(State(state): State<RuntimeState>, headers: HeaderMap) -> Response {
    if !authorize(&headers, &state) {
        return unauthorized();
    }
    let sock = state.admin.reload_sock.clone();
    let handle = tokio::spawn(async move {
        match tokio::net::UnixStream::connect(&sock).await {
            Ok(mut stream) => {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                if let Err(error) = stream.write_all(b"reload\n").await {
                    return format!("error: {error}");
                }
                let mut buf = Vec::new();
                let _ = stream.read_to_end(&mut buf).await;
                String::from_utf8_lossy(&buf).trim().to_string()
            }
            Err(error) => format!("error: {error}"),
        }
    });
    let reply = handle.await.unwrap_or_else(|_| "error: cancelled".into());
    let _ = state.admin.db.record_operation(
        "reload",
        &json!({}),
        if reply.starts_with("error") {
            Err(reply.clone())
        } else {
            Ok(json!({ "ok": true }))
        },
    );
    if reply == "ok" {
        Json(json!({ "ok": true })).into_response()
    } else {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": reply })),
        )
            .into_response()
    }
}

async fn not_configured() -> Response {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({ "ok": false, "error": "not_configured" })),
    )
        .into_response()
}

async fn fallback(State(state): State<RuntimeState>, req: Request<Body>) -> Response {
    let path = req.uri().path().to_string();
    if path.starts_with("/admin/api/") {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "ok": false, "error": "not_found" })),
        )
            .into_response();
    }
    serve_ui(&state, &path).await
}

async fn serve_ui(state: &RuntimeState, path: &str) -> Response {
    let relative = if path == "/admin" || path == "/admin/" || path.starts_with("/admin/sessions/")
    {
        "index.html"
    } else if let Some(rest) = path.strip_prefix("/admin/assets/") {
        rest
    } else if path == "/admin/assets" {
        ""
    } else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "ok": false, "error": "not_found" })),
        )
            .into_response();
    };
    let full = if relative == "index.html" {
        state.admin.ui_dir.join("index.html")
    } else {
        state.admin.ui_dir.join("assets").join(relative)
    };
    match tokio::fs::read(&full).await {
        Ok(bytes) => (
            StatusCode::OK,
            [(axum::http::header::CONTENT_TYPE, content_type_for(&full))],
            bytes,
        )
            .into_response(),
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "ok": false, "error": "not_found" })),
        )
            .into_response(),
    }
}

fn content_type_for(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
    {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "json" => "application/json",
        _ => "application/octet-stream",
    }
}

fn authorize(headers: &HeaderMap, state: &RuntimeState) -> bool {
    let Some(token) = &state.admin.admin_token else {
        return true;
    };
    headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim().trim_start_matches("Bearer ").trim())
        .is_some_and(|value| value == token)
}

fn redact_operation_input(value: &Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, value)| {
                    let normalized = key.to_ascii_lowercase().replace('-', "_");
                    let value = if matches!(
                        normalized.as_str(),
                        "auth"
                            | "auth_json_content"
                            | "headers"
                            | "githubtoken"
                            | "github_token"
                            | "app_token"
                            | "bot_token"
                    ) {
                        Value::String("[redacted]".to_owned())
                    } else {
                        redact_operation_input(value)
                    };
                    (key.clone(), value)
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(redact_operation_input).collect()),
        _ => value.clone(),
    }
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({ "ok": false, "error": "unauthorized" })),
    )
        .into_response()
}

#[allow(unused)]
fn unused(_: Path<String>, _: Query<Value>) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_inputs_redact_profile_and_github_secrets() {
        let redacted = redact_operation_input(&json!({
            "auth": { "key": "profile-secret" },
            "headers": { "authorization": "header-secret" },
            "nested": [{ "githubToken": "github-secret", "name": "kept" }],
        }));

        assert_eq!(redacted["auth"], "[redacted]");
        assert_eq!(redacted["headers"], "[redacted]");
        assert_eq!(redacted["nested"][0]["githubToken"], "[redacted]");
        assert_eq!(redacted["nested"][0]["name"], "kept");
        let encoded = redacted.to_string();
        assert!(!encoded.contains("profile-secret"));
        assert!(!encoded.contains("header-secret"));
        assert!(!encoded.contains("github-secret"));
    }
}
