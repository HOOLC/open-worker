//! Provider catalogs. Missing execution metadata stays unconfigured; enumerating
//! a model never invents its token limits or enables an incomplete configuration.
use crate::{providers, ProfileDocument, ProfileModel, ProfilePaths, ProfileView};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize)]
pub struct DiscoveredModel {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration: Option<ProfileModel>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct ModelDiscovery {
    pub supported: bool,
    pub items: Vec<DiscoveredModel>,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ModelUpdate {
    pub profile: ProfileView,
    pub added: usize,
    pub configured: usize,
    pub truncated: bool,
}

pub async fn discover_models(
    paths: &impl ProfilePaths,
    profile_id: &str,
) -> Result<ModelDiscovery> {
    let mut catalog = fetch_catalog(paths, profile_id).await?.0;
    // Preserve the read-only ID-list contract for existing clients.
    for model in &mut catalog.items {
        model.configuration = None;
    }
    Ok(catalog)
}

pub async fn refresh_models(paths: &impl ProfilePaths, profile_id: &str) -> Result<ModelUpdate> {
    let (catalog, queried) = fetch_catalog(paths, profile_id).await?;
    anyhow::ensure!(
        catalog.supported,
        "model discovery is not supported for this connection"
    );
    let _guard = crate::storage::lock_profile(paths, profile_id)?;
    let mut document = crate::read_profile(paths, profile_id)?;
    anyhow::ensure!(
        document.provider == queried.provider
            && document.billing == queried.billing
            && document.base_url == queried.base_url
            && document.headers == queried.headers
            && document.auth == queried.auth,
        "connection changed during model discovery; retry the update"
    );
    let previous = document.clone();
    let mut added = 0;
    let mut configured = 0;
    let mut truncated = catalog.truncated;
    for item in catalog.items {
        let Some(configuration) = item.configuration else {
            continue;
        };
        if let Some(existing) = document.models.iter_mut().find(|model| model.id == item.id) {
            // Do not undo a manual edit or reactivate an explicitly disabled model.
            if existing.limits.is_none() && configuration.limits.is_some() {
                existing.limits = configuration.limits;
                configured += 1;
            }
        } else {
            if document.models.len() >= 500 {
                truncated = true;
                continue;
            }
            if configuration.limits.is_some() {
                configured += 1;
            }
            document.models.push(configuration);
            added += 1;
        }
    }
    if previous.models.is_empty() && !document.models.iter().any(|model| model.default) {
        if let Some(model) = document.models.iter_mut().find(|model| model.enabled) {
            model.default = true;
        }
    }
    if document != previous {
        crate::app::write_unlocked(paths, profile_id, &document)?;
    }
    Ok(ModelUpdate {
        profile: crate::app::view(profile_id, &document),
        added,
        configured,
        truncated,
    })
}

async fn fetch_catalog(
    paths: &impl ProfilePaths,
    profile_id: &str,
) -> Result<(ModelDiscovery, ProfileDocument)> {
    let mut document = crate::read_profile(paths, profile_id)?;
    let provider = providers::get(&document.provider)?;
    let template = provider.template(&document.billing)?;
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let refreshed = provider
        .refresh_if_needed(&http, document.auth.clone())
        .await?;
    if refreshed != document.auth {
        anyhow::ensure!(
            crate::app::commit_probe_auth(
                paths,
                profile_id,
                &document.auth,
                Some(refreshed.clone())
            )?,
            "credentials changed during model discovery"
        );
        document.auth = refreshed;
    }
    let mut headers = reqwest::header::HeaderMap::new();
    let mut decorated = serde_json::to_value(&document)?;
    provider.decorate_document(&mut decorated);
    let endpoint = decorated["base_url"]
        .as_str()
        .or(document.base_url.as_deref())
        .unwrap_or(&template.base_url)
        .trim_end_matches('/');
    for values in [
        template.headers.as_object(),
        decorated["headers"].as_object(),
    ]
    .into_iter()
    .flatten()
    {
        for (key, value) in values {
            if let Some(value) = value.as_str() {
                headers.insert(
                    reqwest::header::HeaderName::from_bytes(key.as_bytes())?,
                    value.parse()?,
                );
            }
        }
    }
    let mut request = http.get(format!("{endpoint}/models")).headers(headers);
    let credential = provider.bearer(&document.auth)?;
    if document.provider == "anthropic" {
        request = request
            .header("anthropic-version", "2023-06-01")
            .query(&[("limit", "1000")]);
        request = if document.billing == "subscription" {
            request.bearer_auth(credential)
        } else {
            request.header("x-api-key", credential)
        };
    } else {
        request = request.bearer_auth(credential);
    }
    if document.provider == "openai" && document.billing == "subscription" {
        // Catalog protocol compatibility, independent of Zork's product version.
        request = request.query(&[("client_version", "0.144.1")]);
    }
    let mut response = request
        .send()
        .await
        .context("model discovery request failed")?;
    if matches!(
        response.status(),
        reqwest::StatusCode::NOT_FOUND | reqwest::StatusCode::METHOD_NOT_ALLOWED
    ) {
        return Ok((
            ModelDiscovery {
                supported: false,
                items: vec![],
                truncated: false,
                reason: Some(
                    "This connection does not expose a model-list API; add models manually".into(),
                ),
            },
            document,
        ));
    }
    anyhow::ensure!(
        response.status().is_success(),
        "provider model-list request returned {}",
        response.status()
    );
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        anyhow::ensure!(
            bytes.len() + chunk.len() <= 4 * 1024 * 1024,
            "provider model list is too large"
        );
        bytes.extend_from_slice(&chunk);
    }
    let value: Value = serde_json::from_slice(&bytes)?;
    let catalog = parse_models_for(value, Some((&document, &template)))?;
    Ok((catalog, document))
}
#[cfg(test)]
fn parse_models(value: Value) -> Result<ModelDiscovery> {
    parse_models_for(value, None)
}
fn parse_models_for(
    value: Value,
    context: Option<(&ProfileDocument, &providers::ProfileTemplate)>,
) -> Result<ModelDiscovery> {
    let rows = value["data"]
        .as_array()
        .or_else(|| value["models"].as_array())
        .context("provider returned no model list")?;
    let mut ids = std::collections::BTreeMap::new();
    for row in rows.iter().take(1000) {
        if row["visibility"] == "hide"
            || row["hidden"] == true
            || row.pointer("/_meta/hidden") == Some(&Value::Bool(true))
        {
            continue;
        }
        if row["output_modalities"]
            .as_array()
            .is_some_and(|items| !items.iter().any(|item| item == "text"))
        {
            continue;
        }
        if let Some(id) = row["model"]
            .as_str()
            .or_else(|| row["modelId"].as_str())
            .or_else(|| row["slug"].as_str())
            .or_else(|| row["id"].as_str())
            .filter(|id| {
                !id.trim().is_empty() && id.len() <= 256 && !id.chars().any(char::is_control)
            })
        {
            ids.entry(id.to_owned()).or_insert_with(|| {
                context.map(|(document, template)| configuration(document, template, id, row))
            });
        }
    }
    Ok(ModelDiscovery {
        supported: true,
        items: ids
            .into_iter()
            .map(|(id, configuration)| DiscoveredModel { id, configuration })
            .collect(),
        truncated: rows.len() > 1000 || value["has_more"] == true,
        reason: None,
    })
}
fn configuration(
    document: &ProfileDocument,
    template: &providers::ProfileTemplate,
    id: &str,
    row: &Value,
) -> ProfileModel {
    let known = template
        .models
        .as_array()
        .and_then(|models| models.iter().find(|model| model["id"] == id));
    let api = if document.provider == "anthropic" {
        "anthropic-messages"
    } else if document.provider == "openai" && document.billing == "subscription" {
        "openai-codex-responses"
    } else if row["apiBackend"] == "responses" || row["api_backend"] == "responses" {
        "openai-responses"
    } else {
        template
            .models
            .as_array()
            .and_then(|models| models.first())
            .and_then(|model| model["api"].as_str())
            .unwrap_or("openai-completions")
    };
    let mut model: ProfileModel = serde_json::from_value(known.cloned().unwrap_or_else(||serde_json::json!({
        "id":id,"api":api,"thinking":["off"],"default_thinking":"off","capabilities":{"input":["text"]},"default":false
    }))).expect("provider model template");
    model.default = false;
    let number = |paths: &[&str]| {
        paths
            .iter()
            .find_map(|path| row.pointer(path).and_then(Value::as_u64).filter(|n| *n > 0))
    };
    let context = number(&[
        "/max_input_tokens",
        "/context_window",
        "/contextWindow",
        "/context_length",
        "/_meta/contextWindow",
        "/_meta/totalContextTokens",
    ])
    .or_else(|| {
        model
            .limits
            .as_ref()
            .map(|limits| limits.context_window_tokens)
    });
    let output = number(&[
        "/max_tokens",
        "/max_output_tokens",
        "/max_completion_tokens",
        "/maxCompletionTokens",
    ])
    .and_then(|n| u32::try_from(n).ok())
    .or_else(|| model.limits.as_ref().map(|limits| limits.max_output_tokens));
    model.limits = context
        .zip(output)
        .filter(|(c, o)| *c > u64::from(*o))
        .map(|(c, o)| crate::ModelLimits {
            context_window_tokens: c,
            max_output_tokens: o,
            reserve_percent: 10,
        });
    if let Some(input) = row["input_modalities"].as_array() {
        let input = input
            .iter()
            .filter_map(Value::as_str)
            .filter(|s| matches!(*s, "text" | "image"))
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if !input.is_empty() {
            model.capabilities.input = input;
        }
    } else if row.pointer("/capabilities/image_input/supported") == Some(&Value::Bool(true)) {
        model.capabilities.input = vec!["text".into(), "image".into()];
    }
    if let Some(levels) = row["supported_reasoning_levels"].as_array() {
        let mut levels = levels
            .iter()
            .filter_map(|level| level["effort"].as_str().or_else(|| level.as_str()))
            .filter(|level| {
                !level.is_empty() && level.len() < 32 && !level.chars().any(char::is_control)
            })
            .map(|level| {
                if level == "none" {
                    "off".to_owned()
                } else {
                    level.to_owned()
                }
            })
            .collect::<Vec<_>>();
        levels.sort();
        levels.dedup();
        if !levels.is_empty() {
            model.default_thinking = row["default_reasoning_level"]
                .as_str()
                .map(|level| if level == "none" { "off" } else { level })
                .filter(|level| levels.iter().any(|s| s == level))
                .unwrap_or(&levels[0])
                .to_owned();
            model.thinking = levels;
        }
    }
    model.enabled = model.limits.is_some();
    model
}

#[cfg(test)]
mod tests {
    #[test]
    fn provider_metadata_is_whitelisted_and_incomplete_models_stay_disabled() {
        let document: crate::ProfileDocument = serde_json::from_value(
            serde_json::json!({"provider":"xai","billing":"subscription","models":[]}),
        )
        .unwrap();
        let template = crate::providers::get("xai")
            .unwrap()
            .template("subscription")
            .unwrap();
        let result=super::parse_models_for(serde_json::json!({"data":[
            {"id":"ready","contextWindow":32000,"maxCompletionTokens":4096,"apiKey":"never-store","baseUrl":"https://never-follow.invalid"},
            {"id":"unknown","contextWindow":128000}
        ]}),Some((&document,&template))).unwrap();
        let ready = result
            .items
            .iter()
            .find(|model| model.id == "ready")
            .unwrap()
            .configuration
            .as_ref()
            .unwrap();
        assert!(ready.enabled);
        assert_eq!(ready.limits.as_ref().unwrap().context_window_tokens, 32000);
        assert_eq!(ready.limits.as_ref().unwrap().max_output_tokens, 4096);
        let unknown = result
            .items
            .iter()
            .find(|model| model.id == "unknown")
            .unwrap()
            .configuration
            .as_ref()
            .unwrap();
        assert!(!unknown.enabled);
        assert!(unknown.limits.is_none());
        assert!(!serde_json::to_string(&result).unwrap().contains("never-"));
        let document: crate::ProfileDocument = serde_json::from_value(
            serde_json::json!({"provider":"openai","billing":"subscription","models":[]}),
        )
        .unwrap();
        let template = crate::providers::get("openai")
            .unwrap()
            .template("subscription")
            .unwrap();
        let result=super::parse_models_for(serde_json::json!({"models":[{"slug":"visible","visibility":"list","context_window":32000,"max_output_tokens":4096,"supported_reasoning_levels":[{"effort":"low"},{"effort":"high"}],"default_reasoning_level":"high"},{"slug":"internal","visibility":"hide"}]}),Some((&document,&template))).unwrap();
        assert_eq!(result.items.len(), 1);
        let model = result.items[0].configuration.as_ref().unwrap();
        assert_eq!(model.api, crate::ModelApi::OpenaiCodexResponses);
        assert_eq!(model.default_thinking, "high");
    }

    #[test]
    fn enumeration_exposes_only_deduplicated_ids_and_reports_partial_results() {
        let result = super::parse_models(serde_json::json!({"data":[{"id":"b","secret":"never-forward"},{"id":"a"},{"id":"b"},{"id":"\n"}],"has_more":true,"auth":"never-forward"})).unwrap();
        assert_eq!(
            result
                .items
                .iter()
                .map(|m| m.id.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b"]
        );
        assert!(result.truncated);
        let public = serde_json::to_value(&result).unwrap();
        assert!(public["items"][0].get("limits").is_none());
        assert!(!public.to_string().contains("never-forward"));
        assert!(super::parse_models(serde_json::json!({"error":"no"})).is_err());
    }
}
