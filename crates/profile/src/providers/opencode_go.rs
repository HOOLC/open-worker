use std::time::Duration;

use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};

use super::{
    nonempty, AuthProvider, BillingKind, DeviceCode, DeviceCodePoll, ProfileTemplate, ProviderInfo,
    QuotaSnapshot,
};

const API_BASE: &str = "https://opencode.ai/zen/go/v1";

pub struct OpenCodeGo;

#[async_trait]
impl AuthProvider for OpenCodeGo {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: "opencode-go",
            label: "OpenCode Go",
            billing: &[BillingKind {
                id: "subscription",
                label: "OpenCode Go 订阅",
            }],
        }
    }

    fn template(&self, billing: &str) -> Result<ProfileTemplate> {
        match billing {
            "subscription" => Ok(ProfileTemplate {
                provider: "opencode-go".into(),
                billing: "subscription".into(),
                base_url: API_BASE.into(),
                headers: json!({}),
                models: json!([{
                    "id": "muse-spark-1.2-contributor",
                    "api": "openai-responses",
                    "streaming": true,
                    "thinking": ["off", "minimal", "low", "medium", "high", "xhigh"],
                    "default_thinking": "xhigh",
                    "capabilities": { "input": ["text", "image"] },
                    "limits": {
                        "context_window_tokens": 1_048_576,
                        "max_output_tokens": 131_072
                    },
                    "default": true
                }]),
            }),
            other => anyhow::bail!("opencode-go does not support billing {other}"),
        }
    }

    fn bearer(&self, auth: &Value) -> Result<String> {
        nonempty(auth.get("key"))
            .or_else(|| nonempty(auth.get("access")))
            .context("Missing OpenCode Go credential")
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
            .context("OpenCode Go models")?;
        if !response.status().is_success() {
            anyhow::bail!(
                "OpenCode Go model probe failed with status {}",
                response.status().as_u16()
            );
        }
        Ok(QuotaSnapshot {
            account: json!({
                "ok": true,
                "account": { "type": "opencode-go", "planType": "Go" }
            }),
            rate_limits: json!({
                "ok": false,
                "error": "not_reported_by_provider"
            }),
            auth: None,
        })
    }

    async fn refresh_auth(&self, _http: &Client, auth: Value) -> Result<Value> {
        Ok(auth)
    }

    async fn refresh_if_needed(&self, _http: &Client, auth: Value) -> Result<Value> {
        Ok(auth)
    }

    fn supports_device_code(&self, _billing: &str) -> bool {
        false
    }

    async fn start_device_code(&self, _http: &Client) -> Result<DeviceCode> {
        anyhow::bail!("OpenCode Go uses an API key from the OpenCode subscription page")
    }

    async fn poll_device_code(
        &self,
        _http: &Client,
        _pending: &DeviceCode,
    ) -> Result<DeviceCodePoll> {
        anyhow::bail!("OpenCode Go does not use device-code login")
    }
}
