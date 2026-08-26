use anyhow::{Context, Result};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::config::RuntimeConfig;
use crate::db::{GatewayDb, SessionRow};

const SLACK_SYSTEM_PROMPT: &str = include_str!("../prompts/slack-thread-base-instructions.md");

#[derive(Clone, Debug, Deserialize)]
pub struct AgentProfile {
    pub profile_id: String,
    #[serde(default)]
    pub billing: String,
    pub auth_configured: bool,
    #[serde(default)]
    pub account: Value,
    #[serde(default, rename = "rateLimits")]
    pub rate_limits: Value,
    pub models: Vec<AgentModel>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AgentModel {
    pub id: String,
    pub thinking: Vec<String>,
    pub default_thinking: String,
    pub default: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SessionSelection {
    pub profile_id: String,
    pub model: String,
    pub thinking: String,
}

#[derive(Debug, Deserialize)]
struct ProfileList {
    items: Vec<AgentProfile>,
}

#[derive(Debug, Deserialize)]
pub struct CreatedSession {
    pub session_id: String,
    pub workspace: String,
    profile_id: String,
    model: String,
    thinking: String,
}

#[derive(Debug)]
pub struct AgentHttpError {
    pub status: StatusCode,
    pub message: String,
}

impl std::fmt::Display for AgentHttpError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for AgentHttpError {}

pub fn base_url(config: &RuntimeConfig) -> String {
    zork_config::loopback_base_url(&config.agent_bind)
}

pub fn authenticate(
    config: &RuntimeConfig,
    request: reqwest::RequestBuilder,
) -> reqwest::RequestBuilder {
    match &config.agent_token {
        Some(token) => request.bearer_auth(token),
        None => request,
    }
}

pub async fn list_profiles(config: &RuntimeConfig) -> Result<Vec<AgentProfile>> {
    let http = client()?;
    let response = authenticate(
        config,
        http.get(format!("{}/v1/profiles", base_url(config))),
    )
    .send()
    .await
    .context("list zork-agent profiles")?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(json!({}));
    if !status.is_success() {
        anyhow::bail!(
            "zork-agent profile list failed: {}",
            api_error_message(&body).unwrap_or_else(|| status.to_string())
        );
    }
    let list: ProfileList =
        serde_json::from_value(body).context("invalid zork-agent profile list")?;
    Ok(list.items)
}

pub fn default_selection(profiles: &[AgentProfile]) -> Option<SessionSelection> {
    let (model, thinking) = profiles.iter().find_map(|profile| {
        if !profile.auth_configured {
            return None;
        }
        let model = profile.models.iter().find(|model| model.default)?;
        model
            .thinking
            .iter()
            .any(|thinking| thinking == &model.default_thinking)
            .then(|| (model.id.clone(), model.default_thinking.clone()))
    })?;
    resolve_selection(
        profiles,
        &SessionSelection {
            profile_id: "auto".to_owned(),
            model,
            thinking,
        },
    )
}

pub fn resolve_selection(
    profiles: &[AgentProfile],
    requested: &SessionSelection,
) -> Option<SessionSelection> {
    let compatible = profiles
        .iter()
        .filter(|profile| {
            profile.auth_configured
                && profile.models.iter().any(|model| {
                    model.id == requested.model
                        && model
                            .thinking
                            .iter()
                            .any(|thinking| thinking == &requested.thinking)
                })
        })
        .collect::<Vec<_>>();
    let profile = if requested.profile_id == "auto" {
        recommended_profile(&compatible)?
    } else {
        compatible
            .into_iter()
            .find(|profile| profile.profile_id == requested.profile_id)?
    };
    Some(SessionSelection {
        profile_id: profile.profile_id.clone(),
        model: requested.model.clone(),
        thinking: requested.thinking.clone(),
    })
}

pub async fn ensure_session(
    config: &RuntimeConfig,
    db: &GatewayDb,
    session: &SessionRow,
) -> Result<String> {
    if let Some(session_id) = &session.id {
        return Ok(session_id.clone());
    }
    let profiles = list_profiles(config).await?;
    let selection = match default_selection(&profiles) {
        Some(selection) => selection,
        None => {
            db.set_selection_block(&session.key, "no_selectable_profiles")?;
            anyhow::bail!("no selectable Agent profiles");
        }
    };
    let created = create_session(
        config,
        &selection,
        Some(SLACK_SYSTEM_PROMPT),
        &session.workspace_path,
    )
    .await?;
    db.set_agent_session(
        &session.key,
        &created.session_id,
        &created.workspace,
        &selection.profile_id,
        &selection.model,
        &selection.thinking,
    )?;
    Ok(created.session_id)
}

pub async fn create_session(
    config: &RuntimeConfig,
    selection: &SessionSelection,
    system_prompt: Option<&str>,
    workspace: &str,
) -> std::result::Result<CreatedSession, AgentHttpError> {
    let http = client().map_err(|error| AgentHttpError {
        status: StatusCode::BAD_GATEWAY,
        message: error.to_string(),
    })?;
    let response = authenticate(
        config,
        http.post(format!("{}/v1/sessions", base_url(config))),
    )
    .json(&json!({
        "profile_id": selection.profile_id,
        "model": selection.model,
        "thinking": selection.thinking,
        "system_prompt": system_prompt,
        "workspace": workspace,
    }))
    .send()
    .await
    .map_err(|error| AgentHttpError {
        status: StatusCode::BAD_GATEWAY,
        message: format!("create zork-agent session: {error}"),
    })?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(json!({}));
    if status != StatusCode::CREATED {
        return Err(AgentHttpError {
            status: if status.is_client_error() {
                status
            } else {
                StatusCode::BAD_GATEWAY
            },
            message: api_error_message(&body).unwrap_or_else(|| status.to_string()),
        });
    }
    let created: CreatedSession = serde_json::from_value(body).map_err(|error| AgentHttpError {
        status: StatusCode::BAD_GATEWAY,
        message: format!("invalid zork-agent session response: {error}"),
    })?;
    if created.profile_id != selection.profile_id
        || created.model != selection.model
        || created.thinking != selection.thinking
    {
        return Err(AgentHttpError {
            status: StatusCode::BAD_GATEWAY,
            message: "zork-agent returned a different session selection".to_owned(),
        });
    }
    if created.workspace.is_empty() {
        return Err(AgentHttpError {
            status: StatusCode::BAD_GATEWAY,
            message: "zork-agent returned an empty session workspace".to_owned(),
        });
    }
    Ok(created)
}

pub async fn update_selection(
    config: &RuntimeConfig,
    session_id: &str,
    selection: &SessionSelection,
) -> std::result::Result<SessionSelection, AgentHttpError> {
    let http = client().map_err(|error| AgentHttpError {
        status: StatusCode::BAD_GATEWAY,
        message: error.to_string(),
    })?;
    let response = authenticate(
        config,
        http.put(format!(
            "{}/v1/sessions/{session_id}/selection",
            base_url(config)
        )),
    )
    .json(selection)
    .send()
    .await
    .map_err(|error| AgentHttpError {
        status: StatusCode::BAD_GATEWAY,
        message: format!("update zork-agent session selection: {error}"),
    })?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(json!({}));
    if !status.is_success() {
        return Err(AgentHttpError {
            status: if status.is_client_error() {
                status
            } else {
                StatusCode::BAD_GATEWAY
            },
            message: api_error_message(&body).unwrap_or_else(|| status.to_string()),
        });
    }
    let updated: CreatedSession = serde_json::from_value(body).map_err(|error| AgentHttpError {
        status: StatusCode::BAD_GATEWAY,
        message: format!("invalid zork-agent session response: {error}"),
    })?;
    if updated.session_id != session_id
        || updated.profile_id != selection.profile_id
        || updated.model != selection.model
        || updated.thinking != selection.thinking
    {
        return Err(AgentHttpError {
            status: StatusCode::BAD_GATEWAY,
            message: "zork-agent returned a different session selection".to_owned(),
        });
    }
    Ok(SessionSelection {
        profile_id: updated.profile_id,
        model: updated.model,
        thinking: updated.thinking,
    })
}

pub async fn append_mailbox(config: &RuntimeConfig, session_id: &str, content: &str) -> Result<()> {
    let http = client()?;
    let response = authenticate(
        config,
        http.post(format!(
            "{}/v1/sessions/{session_id}/mailbox",
            base_url(config)
        )),
    )
    .json(&json!({ "content": content }))
    .send()
    .await
    .context("append zork-agent mailbox")?;
    if response.status() != StatusCode::ACCEPTED {
        let status = response.status();
        let body: Value = response.json().await.unwrap_or(json!({}));
        anyhow::bail!(
            "zork-agent mailbox append failed: {}",
            api_error_message(&body).unwrap_or_else(|| status.to_string())
        );
    }
    Ok(())
}

pub async fn cancel_session(config: &RuntimeConfig, session_id: &str) -> Result<bool> {
    let response = authenticate(
        config,
        client()?.post(format!(
            "{}/v1/sessions/{session_id}/cancel",
            base_url(config)
        )),
    )
    .send()
    .await
    .context("cancel zork-agent session")?;
    match response.status() {
        StatusCode::NO_CONTENT => Ok(true),
        StatusCode::NOT_FOUND => Ok(false),
        status => {
            let body: Value = response.json().await.unwrap_or(json!({}));
            anyhow::bail!(
                "zork-agent cancellation failed: {}",
                api_error_message(&body).unwrap_or_else(|| status.to_string())
            )
        }
    }
}

pub async fn put_profile(
    config: &RuntimeConfig,
    profile_id: &str,
    document: &Value,
) -> std::result::Result<Value, AgentHttpError> {
    let http = client().map_err(|error| AgentHttpError {
        status: StatusCode::BAD_GATEWAY,
        message: error.to_string(),
    })?;
    let response = authenticate(
        config,
        http.put(format!("{}/v1/profiles/{profile_id}", base_url(config))),
    )
    .json(document)
    .send()
    .await
    .map_err(|error| AgentHttpError {
        status: StatusCode::BAD_GATEWAY,
        message: format!("put zork-agent profile: {error}"),
    })?;
    response_value_with_status(response).await
}

pub async fn delete_profile(
    config: &RuntimeConfig,
    profile_id: &str,
) -> std::result::Result<(), AgentHttpError> {
    let http = client().map_err(|error| AgentHttpError {
        status: StatusCode::BAD_GATEWAY,
        message: error.to_string(),
    })?;
    let response = authenticate(
        config,
        http.delete(format!("{}/v1/profiles/{profile_id}", base_url(config))),
    )
    .send()
    .await
    .map_err(|error| AgentHttpError {
        status: StatusCode::BAD_GATEWAY,
        message: format!("delete zork-agent profile: {error}"),
    })?;
    if response.status() != StatusCode::NO_CONTENT {
        let status = response.status();
        let body: Value = response.json().await.unwrap_or(json!({}));
        return Err(AgentHttpError {
            status: if status.is_client_error() {
                status
            } else {
                StatusCode::BAD_GATEWAY
            },
            message: api_error_message(&body).unwrap_or_else(|| status.to_string()),
        });
    }
    Ok(())
}

pub async fn profiles_value(config: &RuntimeConfig) -> Result<Value> {
    let response = authenticate(
        config,
        client()?.get(format!("{}/v1/profiles", base_url(config))),
    )
    .send()
    .await
    .context("list zork-agent profiles")?;
    response_value(response, "list zork-agent profiles").await
}

async fn response_value(response: reqwest::Response, operation: &str) -> Result<Value> {
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(json!({}));
    if !status.is_success() {
        anyhow::bail!(
            "{operation} failed: {}",
            api_error_message(&body).unwrap_or_else(|| status.to_string())
        );
    }
    Ok(body)
}

async fn response_value_with_status(
    response: reqwest::Response,
) -> std::result::Result<Value, AgentHttpError> {
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(json!({}));
    if !status.is_success() {
        return Err(AgentHttpError {
            status: if status.is_client_error() {
                status
            } else {
                StatusCode::BAD_GATEWAY
            },
            message: api_error_message(&body).unwrap_or_else(|| status.to_string()),
        });
    }
    Ok(body)
}

fn client() -> Result<Client> {
    Client::builder()
        .no_proxy()
        .build()
        .context("Agent HTTP client")
}

fn api_error_message(body: &Value) -> Option<String> {
    body.get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn recommended_profile<'a>(profiles: &[&'a AgentProfile]) -> Option<&'a AgentProfile> {
    let scored = profiles
        .iter()
        .map(|profile| (*profile, remaining_score(profile)))
        .filter(|(_, score)| *score > 0.0)
        .collect::<Vec<_>>();
    let mut pool = if scored.is_empty() {
        profiles
            .iter()
            .map(|profile| (*profile, 0.0))
            .collect::<Vec<_>>()
    } else {
        let subscriptions = scored
            .iter()
            .copied()
            .filter(|(profile, _)| profile.billing == "subscription")
            .collect::<Vec<_>>();
        if subscriptions.is_empty() {
            scored
        } else {
            subscriptions
        }
    };
    pool.sort_by(|(left, left_score), (right, right_score)| {
        right_score
            .partial_cmp(left_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.profile_id.cmp(&right.profile_id))
    });
    pool.first().map(|(profile, _)| *profile)
}

fn remaining_score(profile: &AgentProfile) -> f64 {
    if profile.account.get("ok").and_then(Value::as_bool) != Some(true)
        || profile.rate_limits.get("ok").and_then(Value::as_bool) != Some(true)
    {
        return 0.0;
    }
    if profile.billing == "usage" {
        let credits = profile.rate_limits.pointer("/rateLimits/credits");
        if credits.and_then(|value| value.get("unlimited").and_then(Value::as_bool)) == Some(true) {
            return 100.0;
        }
        return credits
            .and_then(|value| value.get("balance"))
            .and_then(|value| {
                value
                    .as_f64()
                    .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
            })
            .unwrap_or(0.0);
    }
    let used = profile
        .rate_limits
        .pointer("/rateLimits/secondary/usedPercent")
        .and_then(Value::as_f64)
        .or_else(|| {
            profile
                .rate_limits
                .pointer("/rateLimits/primary/usedPercent")
                .and_then(Value::as_f64)
        })
        .unwrap_or(100.0);
    (100.0 - used).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_only_an_explicit_profile_default_model_and_thinking() {
        let profiles = vec![AgentProfile {
            profile_id: "grok".to_owned(),
            billing: "subscription".to_owned(),
            auth_configured: true,
            account: json!({}),
            rate_limits: json!({}),
            models: vec![AgentModel {
                id: "grok-4.6".to_owned(),
                thinking: vec!["high".to_owned(), "xhigh".to_owned()],
                default_thinking: "xhigh".to_owned(),
                default: true,
            }],
        }];
        assert_eq!(
            default_selection(&profiles),
            Some(SessionSelection {
                profile_id: "grok".to_owned(),
                model: "grok-4.6".to_owned(),
                thinking: "xhigh".to_owned(),
            })
        );
    }

    #[test]
    fn automatic_profile_keeps_the_requested_model_and_thinking() {
        let models = vec![AgentModel {
            id: "grok-4.6".to_owned(),
            thinking: vec!["high".to_owned(), "xhigh".to_owned()],
            default_thinking: "xhigh".to_owned(),
            default: true,
        }];
        let profiles = vec![
            AgentProfile {
                profile_id: "usage".to_owned(),
                billing: "usage".to_owned(),
                auth_configured: true,
                account: json!({ "ok": true }),
                rate_limits: json!({ "ok": true, "rateLimits": { "credits": { "balance": "100" } } }),
                models: models.clone(),
            },
            AgentProfile {
                profile_id: "subscription".to_owned(),
                billing: "subscription".to_owned(),
                auth_configured: true,
                account: json!({ "ok": true }),
                rate_limits: json!({ "ok": true, "rateLimits": { "secondary": { "usedPercent": 60 } } }),
                models,
            },
        ];
        assert_eq!(
            resolve_selection(
                &profiles,
                &SessionSelection {
                    profile_id: "auto".to_owned(),
                    model: "grok-4.6".to_owned(),
                    thinking: "high".to_owned(),
                },
            ),
            Some(SessionSelection {
                profile_id: "subscription".to_owned(),
                model: "grok-4.6".to_owned(),
                thinking: "high".to_owned(),
            })
        );
    }
}
