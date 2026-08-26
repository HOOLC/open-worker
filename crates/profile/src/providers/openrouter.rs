use std::io::Read;
use std::time::Duration;

use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::Utc;
use reqwest::Client;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::{
    nonempty, urlencoding, AuthProvider, BillingKind, DeviceCode, DeviceCodePoll, ProfileTemplate,
    ProviderInfo, QuotaSnapshot,
};

const AUTHORIZE_URL: &str = "https://openrouter.ai/auth";
const TOKEN_URL: &str = "https://openrouter.ai/api/v1/auth/keys";
const API_BASE: &str = "https://openrouter.ai/api/v1";
const CALLBACK_URL: &str = "http://127.0.0.1/oauth/callback";

pub struct OpenRouter;

#[async_trait]
impl AuthProvider for OpenRouter {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: "openrouter",
            label: "OpenRouter",
            billing: &[BillingKind {
                id: "usage",
                label: "API 按量",
            }],
        }
    }

    fn template(&self, billing: &str) -> Result<ProfileTemplate> {
        match billing {
            "usage" => Ok(ProfileTemplate {
                provider: "openrouter".into(),
                billing: "usage".into(),
                base_url: API_BASE.into(),
                headers: json!({}),
                models: json!([
                    {
                        "id": "openrouter/auto",
                        "api": "openai-completions",
                        "streaming": true,
                        "thinking": ["off"],
                        "default_thinking": "off",
                        "capabilities": { "input": ["text", "image"] },
                        "default": true
                    },
                    {
                        "id": "anthropic/claude-sonnet-4",
                        "api": "openai-completions",
                        "streaming": true,
                        "thinking": ["off", "low", "medium", "high"],
                        "default_thinking": "high",
                        "capabilities": { "input": ["text", "image"] }
                    }
                ]),
            }),
            other => anyhow::bail!("openrouter does not support billing {other}"),
        }
    }

    fn bearer(&self, auth: &Value) -> Result<String> {
        nonempty(auth.get("key"))
            .or_else(|| nonempty(auth.get("access")))
            .context("Missing OpenRouter credential")
    }

    async fn probe(&self, http: &Client, document: &Value) -> Result<QuotaSnapshot> {
        let auth = document.get("auth").cloned().unwrap_or(json!({}));
        let key = self.bearer(&auth)?;
        let response = http
            .get(format!("{API_BASE}/models"))
            .header("authorization", format!("Bearer {key}"))
            .timeout(Duration::from_secs(20))
            .send()
            .await
            .context("openrouter models")?;
        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("OpenRouter API key probe failed: {body}");
        }
        Ok(QuotaSnapshot {
            account: json!({
                "ok": true,
                "account": { "type": "openrouter", "planType": "API" },
                "requiresOpenaiAuth": false
            }),
            rate_limits: json!({
                "ok": true,
                "rateLimits": {
                    "limitId": "openrouter_usage",
                    "limitName": "OpenRouter API",
                    "primary": null,
                    "secondary": null,
                    "credits": { "unlimited": true, "balance": null },
                    "planType": "api"
                },
                "rateLimitsByLimitId": {}
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

    fn supports_device_code(&self, billing: &str) -> bool {
        billing == "usage"
    }

    async fn start_device_code(&self, _http: &Client) -> Result<DeviceCode> {
        let (verifier, challenge) = generate_pkce()?;
        let expires_at = Utc::now() + chrono::Duration::seconds(5 * 60);
        Ok(DeviceCode {
            provider: "openrouter".into(),
            billing: "usage".into(),
            device_code: random_id()?,
            user_code: String::new(),
            verification_url: format!(
                "{AUTHORIZE_URL}?callback_url={}&code_challenge={}&code_challenge_method=S256",
                urlencoding(CALLBACK_URL),
                urlencoding(&challenge)
            ),
            interval_seconds: 5,
            expires_at: expires_at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            extra: json!({ "code_verifier": verifier }),
        })
    }

    async fn poll_device_code(
        &self,
        http: &Client,
        pending: &DeviceCode,
    ) -> Result<DeviceCodePoll> {
        if pending.user_code.trim().is_empty() {
            return Ok(DeviceCodePoll::Pending {
                retry_after_seconds: pending.interval_seconds,
            });
        }
        let code = parse_authorization_input(&pending.user_code)?;
        let verifier =
            nonempty(pending.extra.get("code_verifier")).context("missing code_verifier")?;
        let key = exchange_authorization_code(http, &code, &verifier).await?;
        Ok(DeviceCodePoll::Completed {
            auth: json!({
                "type": "api_key",
                "key": key,
            }),
        })
    }
}

async fn exchange_authorization_code(http: &Client, code: &str, verifier: &str) -> Result<String> {
    let response = http
        .post(TOKEN_URL)
        .header("accept", "application/json")
        .header("content-type", "application/json")
        .json(&json!({
            "code": code,
            "code_verifier": verifier,
            "code_challenge_method": "S256",
        }))
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .context("openrouter token exchange")?;
    let status = response.status();
    let payload: Value = response.json().await.unwrap_or(json!({}));
    if !status.is_success() {
        let detail = error_detail(&payload);
        anyhow::bail!(
            "OpenRouter OAuth key exchange failed (HTTP {}){}",
            status.as_u16(),
            detail
                .map(|detail| format!(": {detail}"))
                .unwrap_or_default()
        );
    }
    nonempty(payload.get("key")).context("OpenRouter OAuth response carries no \"key\"")
}

fn parse_authorization_input(input: &str) -> Result<String> {
    let value = input.trim();
    if value.is_empty() {
        anyhow::bail!("Missing authorization code");
    }
    if let Ok(url) = reqwest::Url::parse(value) {
        if let Some((_, description)) = url
            .query_pairs()
            .find(|(key, _)| key == "error_description")
        {
            anyhow::bail!("OpenRouter authorization failed: {description}");
        }
        if let Some((_, error)) = url.query_pairs().find(|(key, _)| key == "error") {
            anyhow::bail!("OpenRouter authorization failed: {error}");
        }
        return url
            .query_pairs()
            .find(|(key, _)| key == "code")
            .map(|(_, code)| code.into_owned())
            .filter(|code| !code.is_empty())
            .context("OpenRouter returned no authorization code");
    }
    if value.contains("code=") {
        let query = value
            .split_once('?')
            .map(|(_, query)| query)
            .unwrap_or(value);
        let query = query.split('#').next().unwrap_or(query);
        for pair in query.split('&') {
            if let Some((key, code)) = pair.split_once('=') {
                if key == "code" && !code.is_empty() {
                    return Ok(code.to_string());
                }
            }
        }
        anyhow::bail!("OpenRouter returned no authorization code");
    }
    Ok(value.to_string())
}

fn error_detail(body: &Value) -> Option<String> {
    nonempty(body.get("error_description"))
        .or_else(|| nonempty(body.get("message")))
        .or_else(|| nonempty(body.get("error")))
        .or_else(|| {
            body.get("error")
                .and_then(|error| nonempty(error.get("message")))
        })
}

fn generate_pkce() -> Result<(String, String)> {
    let verifier = base64url(&random_bytes(32)?);
    let challenge = base64url(&Sha256::digest(verifier.as_bytes()));
    Ok((verifier, challenge))
}

fn random_id() -> Result<String> {
    Ok(hex_encode(&random_bytes(16)?))
}

fn random_bytes(len: usize) -> Result<Vec<u8>> {
    let mut buf = vec![0u8; len];
    std::fs::File::open("/dev/urandom")
        .context("open /dev/urandom")?
        .read_exact(&mut buf)
        .context("read /dev/urandom")?;
    Ok(buf)
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn base64url(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    let mut i = 0;
    while i + 3 <= bytes.len() {
        let n = ((bytes[i] as u32) << 16) | ((bytes[i + 1] as u32) << 8) | (bytes[i + 2] as u32);
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push(TABLE[((n >> 6) & 63) as usize] as char);
        out.push(TABLE[(n & 63) as usize] as char);
        i += 3;
    }
    match bytes.len() - i {
        1 => {
            let n = (bytes[i] as u32) << 16;
            out.push(TABLE[((n >> 18) & 63) as usize] as char);
            out.push(TABLE[((n >> 12) & 63) as usize] as char);
        }
        2 => {
            let n = ((bytes[i] as u32) << 16) | ((bytes[i + 1] as u32) << 8);
            out.push(TABLE[((n >> 18) & 63) as usize] as char);
            out.push(TABLE[((n >> 12) & 63) as usize] as char);
            out.push(TABLE[((n >> 6) & 63) as usize] as char);
        }
        _ => {}
    }
    out
}
