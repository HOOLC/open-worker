use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use tracing::error;

use crate::config::GatewayConfig;

#[derive(Clone)]
pub struct HttpState {
    pub config: GatewayConfig,
    pub http: reqwest::Client,
}

#[derive(Debug, Deserialize)]
struct DownloadQuery {
    url: String,
}

pub fn router(state: HttpState) -> Router {
    Router::new()
        .route("/readyz", get(readyz))
        .route("/slack/download", get(slack_download))
        .route("/slack/{method}", post(slack_method))
        .with_state(state)
}

async fn readyz() -> impl IntoResponse {
    Json(json!({ "ok": true, "service": "zork-gateway" }))
}

async fn slack_method(
    State(state): State<HttpState>,
    Path(method): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let method = method.trim_matches('/');
    if method.is_empty() || method.contains("..") || method.contains('/') {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": "invalid_method" })),
        )
            .into_response();
    }

    let url = format!("{}/{method}", state.config.slack_api_base_url);
    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/x-www-form-urlencoded; charset=utf-8");
    let response = state
        .http
        .post(url)
        .header(
            "authorization",
            format!("Bearer {}", state.config.slack_bot_token),
        )
        .header("content-type", content_type)
        .body(body)
        .send()
        .await;
    match response {
        Ok(response) => slack_response(response).await,
        Err(error) => {
            error!(error = %error, method, "slack proxy failed");
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "ok": false, "error": error.to_string() })),
            )
                .into_response()
        }
    }
}

async fn slack_download(
    State(state): State<HttpState>,
    Query(query): Query<DownloadQuery>,
) -> impl IntoResponse {
    if !is_allowed_slack_download(&query.url, &state.config.slack_api_base_url) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": "invalid_download_url" })),
        )
            .into_response();
    }

    let response = state
        .http
        .get(&query.url)
        .header(
            "authorization",
            format!("Bearer {}", state.config.slack_bot_token),
        )
        .send()
        .await;
    match response {
        Ok(response) => {
            let status =
                StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or("application/octet-stream")
                .to_string();
            match response.bytes().await {
                Ok(bytes) => (status, [(axum::http::header::CONTENT_TYPE, content_type)], bytes)
                    .into_response(),
                Err(error) => {
                    error!(error = %error, "slack download body failed");
                    (
                        StatusCode::BAD_GATEWAY,
                        Json(json!({ "ok": false, "error": error.to_string() })),
                    )
                        .into_response()
                }
            }
        }
        Err(error) => {
            error!(error = %error, "slack download failed");
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "ok": false, "error": error.to_string() })),
            )
                .into_response()
        }
    }
}

async fn slack_response(response: reqwest::Response) -> axum::response::Response {
    let status = StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    match response.json::<serde_json::Value>().await {
        Ok(payload) => (status, Json(payload)).into_response(),
        Err(error) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "ok": false, "error": error.to_string() })),
        )
            .into_response(),
    }
}

fn is_allowed_slack_download(url: &str, slack_api_base_url: &str) -> bool {
    let Ok(target) = reqwest::Url::parse(url) else {
        return false;
    };
    if !matches!(target.scheme(), "https" | "http") {
        return false;
    }
    let host = target.host_str().unwrap_or_default();
    if host == "files.slack.com" || host == "slack-files.com" {
        return true;
    }
    reqwest::Url::parse(slack_api_base_url)
        .ok()
        .and_then(|api| api.host_str().map(str::to_string))
        .is_some_and(|api_host| api_host == host)
}

pub async fn serve(config: GatewayConfig, http: reqwest::Client) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(config.bind_addr).await?;
    tracing::info!(addr = %config.bind_addr, "gateway http listening");
    axum::serve(listener, router(HttpState { config, http })).await?;
    Ok(())
}
