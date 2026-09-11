use std::time::Duration;

use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};

use super::{
    nonempty, AuthProvider, BillingKind, DeviceCode, DeviceCodePoll, ProfileTemplate, ProviderInfo,
    QuotaSnapshot,
};

const API_BASE: &str = "https://api.deepseek.com";

pub struct DeepSeek;

#[async_trait]
impl AuthProvider for DeepSeek {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: "deepseek",
            label: "DeepSeek",
            billing: &[BillingKind {
                id: "usage",
                label: "API 按量",
            }],
        }
    }

    fn template(&self, billing: &str) -> Result<ProfileTemplate> {
        match billing {
            "usage" => Ok(ProfileTemplate {
                provider: "deepseek".into(),
                billing: "usage".into(),
                base_url: API_BASE.into(),
                headers: json!({}),
                // https://api-docs.deepseek.com/quick_start/pricing/
                // https://api-docs.deepseek.com/api/create-response/
                models: json!(["deepseek-v4-pro", "deepseek-v4-flash"]
                    .into_iter()
                    .map(|id| json!({
                        "id": id,
                        "api": "openai-responses",
                        "streaming": true,
                        "parallel_tool_calls": false,
                        "thinking": ["off", "low", "high", "max"],
                        "default_thinking": "high",
                        "capabilities": { "input": ["text"] },
                        "limits": {
                            "context_window_tokens": 1_000_000,
                            "max_output_tokens": 384_000
                        },
                        "default": id == "deepseek-v4-pro"
                    }))
                    .collect::<Vec<_>>()),
            }),
            other => anyhow::bail!("deepseek does not support billing {other}"),
        }
    }

    fn bearer(&self, auth: &Value) -> Result<String> {
        anyhow::ensure!(
            auth.get("type").and_then(Value::as_str) == Some("api_key"),
            "DeepSeek only supports API key authentication"
        );
        nonempty(auth.get("key")).context("Missing DeepSeek API key")
    }

    async fn probe(&self, http: &Client, document: &Value) -> Result<QuotaSnapshot> {
        let auth = document.get("auth").cloned().unwrap_or_else(|| json!({}));
        let bearer = self.bearer(&auth)?;
        let base_url = document
            .get("base_url")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(API_BASE)
            .trim_end_matches('/');
        let response = http
            .get(format!("{base_url}/models"))
            .bearer_auth(bearer)
            .timeout(Duration::from_secs(20))
            .send()
            .await
            .context("DeepSeek models")?;
        if !response.status().is_success() {
            anyhow::bail!(
                "DeepSeek model probe failed with status {}",
                response.status().as_u16()
            );
        }
        Ok(QuotaSnapshot {
            account: json!({
                "ok": true,
                "account": { "type": "deepseek", "planType": "API" }
            }),
            rate_limits: json!({
                "ok": true,
                "reported": false
            }),
            auth: None,
        })
    }

    async fn refresh_auth(&self, _http: &Client, auth: Value) -> Result<Value> {
        self.bearer(&auth)?;
        Ok(auth)
    }

    async fn refresh_if_needed(&self, _http: &Client, auth: Value) -> Result<Value> {
        self.bearer(&auth)?;
        Ok(auth)
    }

    fn supports_device_code(&self, _billing: &str) -> bool {
        false
    }

    async fn start_device_code(&self, _http: &Client) -> Result<DeviceCode> {
        anyhow::bail!("DeepSeek uses an API key configured on the profile")
    }

    async fn poll_device_code(
        &self,
        _http: &Client,
        _pending: &DeviceCode,
    ) -> Result<DeviceCodePoll> {
        anyhow::bail!("DeepSeek does not use device-code login")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_accepts_nonempty_api_keys() {
        assert_eq!(
            DeepSeek
                .bearer(&json!({"type": "api_key", "key": " secret "}))
                .unwrap(),
            "secret"
        );
        for auth in [
            json!({}),
            json!({"type": "api_key", "key": "  "}),
            json!({"type": "oauth", "access": "token", "key": "secret"}),
            json!({"type": "api_key", "access": "token"}),
        ] {
            assert!(DeepSeek.bearer(&auth).is_err());
        }
    }

    #[test]
    fn catalog_exposes_only_api_billing_without_device_login() {
        let catalog = crate::list_providers();
        let provider = catalog["providers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["id"] == "deepseek")
            .unwrap();
        assert_eq!(provider["billing"].as_array().unwrap().len(), 1);
        assert_eq!(provider["billing"][0]["id"], "usage");
        assert_eq!(provider["billing"][0]["deviceCode"], false);
        assert!(DeepSeek.template("subscription").is_err());
        assert!(!DeepSeek.supports_device_code("subscription"));
    }
}
