use std::convert::Infallible;
use std::sync::Arc;

use async_stream::stream;
use axum::body::to_bytes;
use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{DefaultBodyLimit, Path, Query, Request, State};
use axum::http::header::AUTHORIZATION;
use axum::http::{StatusCode, Uri};
use axum::middleware::{self, Next};
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::broadcast;

use crate::session::runtime::{
    Runtime, RuntimeCommandError, RuntimeStreamEvent, StoreError, StorePort, MAX_SESSION_LIST_LIMIT,
};
use crate::session::state::{SessionEvent, SessionSelection, SessionState, TranscriptRole};
use crate::session::store::JsonlEventStore;

const MAX_BODY_BYTES: usize = 512 * 1024;
const DEFAULT_MESSAGE_LIMIT: usize = 50;
const MAX_MESSAGE_LIMIT: usize = 200;
const MESSAGE_EVENT_SCAN_PAGE: usize = 512;

#[derive(Clone)]
pub struct AppState {
    runtime: Arc<Runtime>,
    store: Arc<JsonlEventStore>,
    profiles: Arc<crate::ProfileStore>,
    agent_token: Option<Arc<str>>,
}

impl AppState {
    pub fn new(
        runtime: Arc<Runtime>,
        store: Arc<JsonlEventStore>,
        profiles: Arc<crate::ProfileStore>,
        agent_token: Option<String>,
    ) -> Self {
        Self {
            runtime,
            store,
            profiles,
            agent_token: agent_token.map(Arc::<str>::from),
        }
    }
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "invalid_request",
            message: message.into(),
        }
    }

    fn invalid(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "invalid_request",
            message: message.into(),
        }
    }

    fn not_found(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code,
            message: message.into(),
        }
    }

    fn internal() -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "internal_error",
            message: "internal error".to_owned(),
        }
    }

    fn from_runtime(error: RuntimeCommandError) -> Self {
        match error {
            RuntimeCommandError::NotFound => {
                Self::not_found("session_not_found", "session not found")
            }
            RuntimeCommandError::Conflict => Self {
                status: StatusCode::CONFLICT,
                code: "conflict",
                message: "session operation conflicts".to_owned(),
            },
            RuntimeCommandError::Invalid(message) => Self::invalid(message),
            RuntimeCommandError::ProfileUnavailable => {
                Self::invalid("profile, model, or thinking is unavailable")
            }
            RuntimeCommandError::Backend => Self::internal(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({
                "error": {
                    "code": self.code,
                    "message": self.message
                }
            })),
        )
            .into_response()
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateSessionRequest {
    profile_id: String,
    model: String,
    thinking: String,
    system_prompt: Option<String>,
    workspace: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectionRequest {
    profile_id: String,
    model: String,
    thinking: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MailboxRequest {
    content: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MessageQuery {
    before: Option<String>,
    limit: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum PublicMessage {
    Message { role: PublicRole, content: String },
    Wait { reason: String },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum PublicRole {
    User,
    Assistant,
    Tool,
}

pub fn router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/v1/profiles", get(list_profiles))
        .route(
            "/v1/profiles/{profile_id}",
            get(get_profile).put(put_profile).delete(delete_profile),
        )
        .route("/v1/sessions", get(list_sessions).post(create_session))
        .route(
            "/v1/sessions/{session_id}",
            get(get_session).delete(delete_session),
        )
        .route(
            "/v1/sessions/{session_id}/selection",
            axum::routing::put(update_session_selection),
        )
        .route("/v1/sessions/{session_id}/mailbox", post(append_mailbox))
        .route("/v1/sessions/{session_id}/messages", get(list_messages))
        .route("/v1/sessions/{session_id}/events", get(stream_events))
        .route("/v1/sessions/{session_id}/cancel", post(cancel_session))
        .layer(DefaultBodyLimit::disable());
    let protected = match state.agent_token.clone() {
        Some(token) => protected.route_layer(middleware::from_fn_with_state(token, require_auth)),
        None => protected,
    };
    Router::new().merge(protected).with_state(state)
}

async fn require_auth(State(expected): State<Arc<str>>, request: Request, next: Next) -> Response {
    let supplied = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    if supplied != Some(expected.as_ref()) {
        return ApiError {
            status: StatusCode::UNAUTHORIZED,
            code: "unauthorized",
            message: "authentication required".to_owned(),
        }
        .into_response();
    }
    next.run(request).await
}

async fn list_profiles(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let profiles = state.profiles.clone();
    let items = tokio::task::spawn_blocking(move || profiles.list())
        .await
        .map_err(|_| ApiError::internal())?
        .map_err(|_| ApiError::internal())?;
    Ok(Json(json!({ "items": items })))
}

async fn get_profile(
    State(state): State<AppState>,
    Path(profile_id): Path<String>,
) -> Result<Json<zork_profile::ProfileView>, ApiError> {
    let profiles = state.profiles.clone();
    let lookup_id = profile_id.clone();
    let profile = tokio::task::spawn_blocking(move || profiles.get(&lookup_id))
        .await
        .map_err(|_| ApiError::internal())?
        .map_err(|_| ApiError::internal())?
        .ok_or_else(|| {
            ApiError::not_found(
                "profile_not_found",
                format!("profile {profile_id} not found"),
            )
        })?;
    Ok(Json(profile))
}

async fn put_profile(
    State(state): State<AppState>,
    Path(profile_id): Path<String>,
    body: Result<Json<Value>, JsonRejection>,
) -> Result<Json<zork_profile::ProfileView>, ApiError> {
    let Json(body) = body.map_err(json_rejection)?;
    let profiles = state.profiles.clone();
    let write_id = profile_id.clone();
    tokio::task::spawn_blocking(move || profiles.put(&write_id, body))
        .await
        .map_err(|_| ApiError::internal())?
        .map_err(|error| ApiError::invalid(error.to_string()))?;
    state
        .profiles
        .refresh_status(&profile_id)
        .await
        .map_err(|_| ApiError::internal())?;
    let profiles = state.profiles.clone();
    let read_id = profile_id.clone();
    let profile = tokio::task::spawn_blocking(move || profiles.get(&read_id))
        .await
        .map_err(|_| ApiError::internal())?
        .map_err(|_| ApiError::internal())?
        .ok_or_else(|| {
            ApiError::not_found(
                "profile_not_found",
                format!("profile {profile_id} not found"),
            )
        })?;
    Ok(Json(profile))
}

async fn delete_profile(
    State(state): State<AppState>,
    Path(profile_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let profiles = state.profiles.clone();
    tokio::task::spawn_blocking(move || profiles.delete(&profile_id))
        .await
        .map_err(|_| ApiError::internal())?
        .map_err(|_| ApiError::internal())?;
    Ok(StatusCode::NO_CONTENT)
}

async fn create_session(
    State(state): State<AppState>,
    body: Result<Json<CreateSessionRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Json(request) = body.map_err(json_rejection)?;
    if request.profile_id.trim().is_empty()
        || request.model.trim().is_empty()
        || request.thinking.trim().is_empty()
    {
        return Err(ApiError::invalid(
            "profile_id, model, and thinking are required",
        ));
    }
    let selection = SessionSelection {
        profile_id: request.profile_id,
        model: request.model,
        thinking: request.thinking,
    };
    let created = state
        .runtime
        .create_session(selection, request.system_prompt, request.workspace)
        .await
        .map_err(ApiError::from_runtime)?;
    Ok((StatusCode::CREATED, Json(session_view(&created.state)?)))
}

async fn list_sessions(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let mut cursor = None;
    let mut items = Vec::new();
    loop {
        let page = state
            .runtime
            .list_sessions(cursor, MAX_SESSION_LIST_LIMIT)
            .await
            .map_err(ApiError::from_runtime)?;
        for item in page.items {
            items.push(session_summary(
                item.session_id,
                item.selection,
                item.workspace,
                if item.status == "idle" {
                    "wait"
                } else {
                    "working"
                },
            )?);
        }
        let Some(next) = page.next_cursor else {
            break;
        };
        cursor = Some(next);
    }
    Ok(Json(json!({ "items": items })))
}

async fn get_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let session = state
        .runtime
        .get_session(session_id)
        .await
        .map_err(ApiError::from_runtime)?;
    Ok(Json(session_view(&session)?))
}

async fn update_session_selection(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    body: Result<Json<SelectionRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(request) = body.map_err(json_rejection)?;
    if request.profile_id.trim().is_empty()
        || request.model.trim().is_empty()
        || request.thinking.trim().is_empty()
    {
        return Err(ApiError::invalid(
            "profile_id, model, and thinking are required",
        ));
    }
    let session = state
        .runtime
        .update_selection(
            session_id,
            SessionSelection {
                profile_id: request.profile_id,
                model: request.model,
                thinking: request.thinking,
            },
        )
        .await
        .map_err(ApiError::from_runtime)?;
    Ok(Json(session_view(&session)?))
}

async fn delete_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state
        .runtime
        .get_session(session_id.clone())
        .await
        .map_err(ApiError::from_runtime)?;
    let _ = state
        .runtime
        .cancel_session(session_id.clone(), "session deleted".to_owned())
        .await
        .map_err(ApiError::from_runtime)?;
    let store = state.store.clone();
    tokio::task::spawn_blocking(move || store.delete_session(&session_id))
        .await
        .map_err(|_| ApiError::internal())?
        .map_err(|error| match error {
            StoreError::SessionNotFound => {
                ApiError::not_found("session_not_found", "session not found")
            }
            _ => ApiError::internal(),
        })?;
    Ok(StatusCode::NO_CONTENT)
}

async fn cancel_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    request: Request,
) -> Result<StatusCode, ApiError> {
    let bytes = to_bytes(request.into_body(), MAX_BODY_BYTES)
        .await
        .map_err(|_| ApiError::invalid("cancel does not accept a request body"))?;
    if !bytes.is_empty() {
        return Err(ApiError::invalid("cancel does not accept a request body"));
    }
    let _ = state
        .runtime
        .cancel_session(session_id, "cancelled".to_owned())
        .await
        .map_err(ApiError::from_runtime)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn append_mailbox(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    body: Result<Json<MailboxRequest>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let Json(request) = body.map_err(json_rejection)?;
    if request.content.is_empty() {
        return Err(ApiError::invalid("content is required"));
    }
    state
        .runtime
        .append_mailbox(session_id, request.content)
        .await
        .map_err(ApiError::from_runtime)?;
    Ok(StatusCode::ACCEPTED)
}

async fn list_messages(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    query: Result<Query<MessageQuery>, QueryRejection>,
) -> Result<Json<Value>, ApiError> {
    let Query(query) = query.map_err(|_| ApiError::bad_request("invalid message query"))?;
    let limit = query.limit.unwrap_or(DEFAULT_MESSAGE_LIMIT);
    if !(1..=MAX_MESSAGE_LIMIT).contains(&limit) {
        return Err(ApiError::invalid(format!(
            "limit must be between 1 and {MAX_MESSAGE_LIMIT}"
        )));
    }
    let before_event_id = match query.before.as_deref() {
        Some(cursor) => Some(
            decode_message_cursor(cursor)
                .ok_or_else(|| ApiError::invalid("invalid message cursor"))?,
        ),
        None => None,
    };
    let store = state.store.clone();
    let (items, older_event_id) = tokio::task::spawn_blocking(move || {
        read_public_message_page(store.as_ref(), &session_id, before_event_id, limit)
    })
    .await
    .map_err(|_| ApiError::internal())?
    .map_err(message_store_error)?;
    let older_cursor = older_event_id.as_deref().map(encode_message_cursor);
    Ok(Json(json!({
        "items": items,
        "older_cursor": older_cursor
    })))
}

async fn stream_events(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<Sse<impl futures_util::Stream<Item = Result<SseEvent, Infallible>>>, ApiError> {
    state
        .runtime
        .get_session(session_id.clone())
        .await
        .map_err(ApiError::from_runtime)?;
    let mut subscription = state.runtime.stream_publisher().subscribe();
    let stream = stream! {
        loop {
            match subscription.receiver.recv().await {
                Ok(message) => match message.event {
                    RuntimeStreamEvent::Durable(record) if record.stream_id == session_id => {
                        if let Some(item) = public_message(&record.event) {
                            let event_name = match item {
                                PublicMessage::Message { .. } => "message",
                                PublicMessage::Wait { .. } => "wait",
                            };
                            if let Ok(data) = serde_json::to_string(&item) {
                                yield Ok(SseEvent::default().event(event_name).data(data));
                            }
                        }
                    }
                    RuntimeStreamEvent::Transient(delta) if delta.session_id == session_id => {
                        let data = json!({ "text": delta.text });
                        yield Ok(SseEvent::default().event("assistant_delta").data(data.to_string()));
                    }
                    _ => {}
                },
                Err(broadcast::error::RecvError::Lagged(_))
                | Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    };
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

fn session_view(state: &SessionState) -> Result<Value, ApiError> {
    session_summary(
        state.session_id.clone(),
        state.selection.clone(),
        state.workspace.clone(),
        if state.work_status() == "idle" {
            "wait"
        } else {
            "working"
        },
    )
}

fn session_summary(
    session_id: String,
    selection: SessionSelection,
    workspace: String,
    status: &'static str,
) -> Result<Value, ApiError> {
    Ok(json!({
        "session_id": session_id,
        "profile_id": selection.profile_id,
        "model": selection.model,
        "thinking": selection.thinking,
        "workspace": workspace,
        "status": status
    }))
}

fn public_message(event: &SessionEvent) -> Option<PublicMessage> {
    match event {
        SessionEvent::MailboxMessageAppended { message } => Some(PublicMessage::Message {
            role: PublicRole::User,
            content: message.content.to_string(),
        }),
        SessionEvent::MessageAppended { message, .. } => match message.role {
            TranscriptRole::Assistant => Some(PublicMessage::Message {
                role: PublicRole::Assistant,
                content: message.content.to_string(),
            }),
            TranscriptRole::Tool => Some(PublicMessage::Message {
                role: PublicRole::Tool,
                content: message.content.to_string(),
            }),
            TranscriptRole::System | TranscriptRole::User => None,
        },
        SessionEvent::WaitSet { wait } => Some(PublicMessage::Wait {
            reason: wait.reason.clone(),
        }),
        _ => None,
    }
}

fn read_public_message_page(
    store: &JsonlEventStore,
    session_id: &str,
    mut before_event_id: Option<String>,
    limit: usize,
) -> Result<(Vec<PublicMessage>, Option<String>), StoreError> {
    let wanted = limit.checked_add(1).ok_or(StoreError::Backend)?;
    let mut newest_first = Vec::with_capacity(wanted);
    while newest_first.len() < wanted {
        let records = store.read_stream_before(
            session_id,
            before_event_id.as_deref(),
            MESSAGE_EVENT_SCAN_PAGE,
        )?;
        let reached_start = records.len() < MESSAGE_EVENT_SCAN_PAGE;
        let Some(oldest_event_id) = records.first().map(|record| record.event_id.clone()) else {
            break;
        };
        before_event_id = Some(oldest_event_id);
        for record in records.iter().rev() {
            if let Some(message) = public_message(&record.event) {
                newest_first.push((record.event_id.clone(), message));
                if newest_first.len() == wanted {
                    break;
                }
            }
        }
        if reached_start {
            break;
        }
    }
    let has_older = newest_first.len() > limit;
    newest_first.truncate(limit);
    let older_event_id = has_older
        .then(|| newest_first.last().map(|(event_id, _)| event_id.clone()))
        .flatten();
    newest_first.reverse();
    Ok((
        newest_first
            .into_iter()
            .map(|(_, message)| message)
            .collect(),
        older_event_id,
    ))
}

fn encode_message_cursor(event_id: &str) -> String {
    format!("m.{event_id}")
}

fn decode_message_cursor(cursor: &str) -> Option<String> {
    let event_id = cursor.strip_prefix("m.")?;
    (!event_id.is_empty()).then(|| event_id.to_owned())
}

fn message_store_error(error: StoreError) -> ApiError {
    match error {
        StoreError::SessionNotFound => {
            ApiError::not_found("session_not_found", "session not found")
        }
        StoreError::InvalidEventId => ApiError::invalid("invalid message cursor"),
        _ => ApiError::internal(),
    }
}

fn json_rejection(rejection: JsonRejection) -> ApiError {
    match rejection {
        JsonRejection::JsonDataError(_) | JsonRejection::MissingJsonContentType(_) => {
            ApiError::invalid("invalid JSON request")
        }
        JsonRejection::JsonSyntaxError(_) => ApiError::bad_request("malformed JSON request"),
        _ => ApiError::bad_request("invalid request body"),
    }
}

#[allow(dead_code)]
async fn not_found(uri: Uri) -> ApiError {
    ApiError::not_found("not_found", format!("route {uri} not found"))
}
