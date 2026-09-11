use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use zork_agent_api::{
    AgentProfile, ApiErrorBody, ApiErrorCode, CreateSessionRequest, EventQuery, ItemList,
    MailboxRequest, MessagePage, MessageQuery, SessionSelection, SessionSummary, SessionView,
    TextDeltaEvent, DURABLE_EVENT_NAME, TEXT_DELTA_EVENT_NAME,
};

use zork_agent::{
    application::{durable_event, Agent, AgentError, UpdateProfileModels},
    session::service::LiveSessionEvent,
};

pub struct AppState {
    pub agent: Agent,
    pub token: Option<String>,
}

pub fn router(state: AppState) -> Router {
    let token = state.token.clone();
    let router = Router::new()
        .route("/profiles", get(list_profiles))
        .route(
            "/profiles/{profile_id}",
            get(get_profile).put(put_profile).delete(delete_profile),
        )
        .route(
            "/profiles/{profile_id}/discovered-models",
            get(discover_models),
        )
        .route(
            "/profiles/{profile_id}/models",
            axum::routing::put(update_profile_models),
        )
        .route("/sessions", get(list_sessions).post(create_session))
        .route(
            "/sessions/{session_id}",
            get(get_session).put(ensure_session).delete(delete_session),
        )
        .route("/sessions/{session_id}/mailbox", post(append_mailbox))
        .route(
            "/sessions/{session_id}/mailbox/{request_id}",
            post(append_mailbox_id),
        )
        .route("/sessions/{session_id}/messages", get(list_messages))
        .route("/sessions/{session_id}/events", get(stream_events))
        .route("/sessions/{session_id}/history", get(list_history))
        .route("/sessions/{session_id}/cancel", post(cancel_session))
        .route(
            "/sessions/{session_id}/selection",
            axum::routing::put(set_selection),
        )
        .route(
            "/sessions/{session_id}/context",
            axum::routing::put(set_context),
        )
        .with_state(Arc::new(state));
    match token {
        Some(token) => router.route_layer(axum::middleware::from_fn_with_state(
            Arc::from(token),
            require_auth,
        )),
        None => router,
    }
}

struct ApiError(AgentError);
impl From<AgentError> for ApiError {
    fn from(error: AgentError) -> Self {
        Self(error)
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let error = self.0;
        let status = match error.code {
            ApiErrorCode::InvalidRequest | ApiErrorCode::SelectionUnavailable => {
                StatusCode::UNPROCESSABLE_ENTITY
            }
            ApiErrorCode::SessionNotFound | ApiErrorCode::ProfileNotFound => StatusCode::NOT_FOUND,
            ApiErrorCode::SessionOverloaded => StatusCode::TOO_MANY_REQUESTS,
            ApiErrorCode::GlobalOverloaded | ApiErrorCode::RunnerCircuitOpen => {
                StatusCode::SERVICE_UNAVAILABLE
            }
            ApiErrorCode::SessionDeleting => StatusCode::CONFLICT,
            ApiErrorCode::InvalidCursor => StatusCode::BAD_REQUEST,
            ApiErrorCode::Unauthorized => StatusCode::UNAUTHORIZED,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, Json(ApiErrorBody::new(error.code, error.message))).into_response()
    }
}

async fn create_session(
    State(state): State<Arc<AppState>>,
    body: Result<Json<CreateSessionRequest>, axum::extract::rejection::JsonRejection>,
) -> Result<(StatusCode, Json<SessionView>), ApiError> {
    let Json(request) = body.map_err(|_| ApiError(AgentError::invalid("invalid JSON body")))?;
    Ok((
        StatusCode::CREATED,
        Json(state.agent.create_session(request).await?),
    ))
}

async fn ensure_session(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
    body: Result<Json<CreateSessionRequest>, axum::extract::rejection::JsonRejection>,
) -> Result<(StatusCode, Json<SessionView>), ApiError> {
    let Json(request) = body.map_err(|_| ApiError(AgentError::invalid("invalid JSON body")))?;
    Ok((
        StatusCode::CREATED,
        Json(state.agent.ensure_session(session_id, request).await?),
    ))
}

async fn append_mailbox_id(
    State(state): State<Arc<AppState>>,
    Path((session_id, request_id)): Path<(String, String)>,
    Json(request): Json<MailboxRequest>,
) -> Result<StatusCode, ApiError> {
    state
        .agent
        .append_mailbox_id(session_id, request_id, request)
        .await?;
    Ok(StatusCode::ACCEPTED)
}

async fn list_sessions(State(state): State<Arc<AppState>>) -> Json<ItemList<SessionSummary>> {
    Json(state.agent.list_sessions().await)
}

async fn get_session(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> Result<Json<SessionView>, ApiError> {
    Ok(Json(state.agent.get_session(session_id).await?))
}

async fn delete_session(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state.agent.delete_session(session_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn append_mailbox(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
    body: Result<Json<MailboxRequest>, axum::extract::rejection::JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let Json(request) = body.map_err(|_| ApiError(AgentError::invalid("invalid JSON body")))?;
    state.agent.append_mailbox(session_id, request).await?;
    Ok(StatusCode::ACCEPTED)
}

async fn cancel_session(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state.agent.cancel_session(session_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn set_selection(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
    body: Result<Json<SessionSelection>, axum::extract::rejection::JsonRejection>,
) -> Result<Json<SessionView>, ApiError> {
    let Json(body) = body.map_err(|_| ApiError(AgentError::invalid("invalid JSON body")))?;
    Ok(Json(state.agent.set_selection(session_id, body).await?))
}

async fn set_context(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
    body: Result<Json<zork_agent_api::ContextConfig>, axum::extract::rejection::JsonRejection>,
) -> Result<Json<SessionView>, ApiError> {
    let Json(config) =
        body.map_err(|_| ApiError(AgentError::invalid("invalid context configuration")))?;
    Ok(Json(state.agent.set_context(session_id, config).await?))
}

async fn list_messages(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
    query: Result<Query<MessageQuery>, axum::extract::rejection::QueryRejection>,
) -> Result<Json<MessagePage>, ApiError> {
    let Query(query) = query.map_err(|_| ApiError(AgentError::invalid("invalid query")))?;
    Ok(Json(state.agent.list_messages(session_id, query).await?))
}

async fn list_history(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
    query: Result<Query<zork_agent_api::HistoryQuery>, axum::extract::rejection::QueryRejection>,
) -> Result<Json<zork_agent_api::HistoryPage<zork_agent::session::events::SessionEvent>>, ApiError>
{
    let Query(query) = query.map_err(|_| ApiError(AgentError::invalid("invalid history query")))?;
    Ok(Json(state.agent.list_history(session_id, query).await?))
}

async fn list_profiles(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ItemList<AgentProfile>>, ApiError> {
    Ok(Json(state.agent.list_profiles().await?))
}

async fn get_profile(
    State(state): State<Arc<AppState>>,
    Path(profile_id): Path<String>,
) -> Result<Json<zork_profile::ProfileView>, ApiError> {
    Ok(Json(state.agent.get_profile(profile_id).await?))
}

async fn put_profile(
    State(state): State<Arc<AppState>>,
    Path(profile_id): Path<String>,
    body: Result<Json<zork_agent_api::ProfileDocument>, axum::extract::rejection::JsonRejection>,
) -> Result<Json<zork_profile::ProfileView>, ApiError> {
    let Json(body) = body.map_err(|_| ApiError(AgentError::invalid("invalid JSON body")))?;
    Ok(Json(state.agent.put_profile(profile_id, body).await?))
}

async fn delete_profile(
    State(state): State<Arc<AppState>>,
    Path(profile_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state.agent.delete_profile(profile_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn discover_models(
    State(state): State<Arc<AppState>>,
    Path(profile_id): Path<String>,
) -> Result<Json<zork_profile::ModelDiscovery>, ApiError> {
    Ok(Json(state.agent.discover_models(profile_id).await?))
}

async fn update_profile_models(
    State(state): State<Arc<AppState>>,
    Path(profile_id): Path<String>,
    body: Result<Json<UpdateProfileModels>, axum::extract::rejection::JsonRejection>,
) -> Result<Json<zork_profile::ProfileView>, ApiError> {
    let Json(body) =
        body.map_err(|_| ApiError(AgentError::invalid("invalid model configuration")))?;
    Ok(Json(
        state.agent.update_profile_models(profile_id, body).await?,
    ))
}

async fn stream_events(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
    query: Result<Query<EventQuery>, axum::extract::rejection::QueryRejection>,
    headers: HeaderMap,
) -> Result<Sse<impl futures_util::Stream<Item = Result<SseEvent, Infallible>>>, ApiError> {
    use futures_util::StreamExt;
    let Query(query) = query.map_err(|_| ApiError(AgentError::invalid("invalid query")))?;
    let cursor = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .filter(|v| !v.is_empty())
        .map(str::to_owned);
    let events = state.agent.events(session_id, cursor, query).await?;
    let wire = async_stream::stream! {
        tokio::pin!(events);
        while let Some(event) = events.next().await {
            let event = match event { Ok(event) => event, Err(_) => break };
            let encoded = match event {
                LiveSessionEvent::Snapshot(snapshot) => serde_json::to_string(snapshot.as_ref()).ok()
                    .map(|data| SseEvent::default().event("snapshot").data(data)),
                LiveSessionEvent::Overview(overview) => {
                    match state.agent.snapshot_from((*overview).clone()).await {
                        Ok(snapshot) => serde_json::to_string(&snapshot).ok().map(|data| SseEvent::default().event("session_updated").data(data)),
                        Err(_) => break,
                    }
                }
                LiveSessionEvent::OutputDelta { session_id, generation, step_id, bytes } =>
                    Some(SseEvent::default().event("output_delta").data(serde_json::json!({"session_id":session_id,"generation":generation,"step_id":step_id,"bytes":bytes}).to_string())),
                LiveSessionEvent::Durable(envelope) => serde_json::to_string(&durable_event(&envelope)).ok()
                    .map(|data| SseEvent::default().event(DURABLE_EVENT_NAME).id(envelope.event_id.clone()).data(data)),
                LiveSessionEvent::TextDelta { session_id, generation, step_id, text } =>
                    serde_json::to_string(&TextDeltaEvent { session_id, generation, step_id, text }).ok()
                        .map(|data| SseEvent::default().event(TEXT_DELTA_EVENT_NAME).data(data)),
            };
            if let Some(event) = encoded { yield Ok(event); }
        }
    };
    Ok(Sse::new(wire).keep_alive(KeepAlive::default()))
}

async fn require_auth(
    State(expected): State<Arc<str>>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    use axum::http::header::AUTHORIZATION;
    let supplied = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    if supplied != Some(expected.as_ref()) {
        return ApiError(AgentError {
            code: ApiErrorCode::Unauthorized,
            message: "authentication required".into(),
        })
        .into_response();
    }
    next.run(request).await
}
