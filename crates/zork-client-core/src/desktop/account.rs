//! Optional Cue OIDC identity. Provider credentials and Mesh device keys are
//! independent; changing the issuer never forwards an existing account token.
use anyhow::{ensure, Context, Result};
use openidconnect::{
    core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata},
    AccessTokenHash, AuthorizationCode, ClientId, CsrfToken, IssuerUrl, Nonce, OAuth2TokenResponse,
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope, TokenResponse,
};
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::Arc,
    time::{Duration, Instant},
};
use zork_config::services::CueAccountConfig;
pub use zork_notify::io::Cancellation as AccountCancellation;

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct AccountIdentity {
    pub issuer: String,
    pub client_id: String,
    pub subject: String,
    pub name: String,
    pub email: Option<String>,
    pub expires_at: i64,
}
pub struct AccountFlow {
    config: CueAccountConfig,
    metadata: CoreProviderMetadata,
    redirect: String,
    verifier: PkceCodeVerifier,
    state: CsrfToken,
    nonce: Nonce,
    listener: TcpListener,
    pub url: String,
    pub cancel: Arc<zork_notify::io::Cancellation>,
}
impl AccountFlow {
    pub fn prepare(
        config: CueAccountConfig,
        cancel: Arc<zork_notify::io::Cancellation>,
    ) -> Result<Self> {
        zork_config::services::validate_cue_redirect(&config.redirect_uri)?;
        ensure!(!cancel.is_cancelled(), "登录已取消");
        let http = http()?;
        let metadata =
            CoreProviderMetadata::discover(&IssuerUrl::new(config.issuer.clone())?, &http)
                .context("无法读取 Cue 登录服务配置")?;
        zork_config::services::validate_endpoint(metadata.authorization_endpoint().as_str())?;
        if let Some(token) = metadata.token_endpoint() {
            zork_config::services::validate_endpoint(token.as_str())?;
        }
        ensure!(!cancel.is_cancelled(), "登录已取消");
        let redirect = config.redirect_uri.clone();
        let port = reqwest::Url::parse(&redirect)?
            .port()
            .context("Cue callback port is required")?;
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
            .context("Cue 登录回调端口被占用，请关闭另一处登录后重试")?;
        listener.set_nonblocking(true)?;
        let client = CoreClient::from_provider_metadata(
            metadata.clone(),
            ClientId::new(config.client_id.clone()),
            None,
        )
        .set_redirect_uri(RedirectUrl::new(redirect.clone())?);
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let (url, state, nonce) = client
            .authorize_url(
                CoreAuthenticationFlow::AuthorizationCode,
                CsrfToken::new_random,
                Nonce::new_random,
            )
            .add_scope(Scope::new("email".into()))
            .add_scope(Scope::new("profile".into()))
            .set_pkce_challenge(challenge)
            .url();
        Ok(Self {
            config,
            metadata,
            redirect,
            verifier,
            state,
            nonce,
            listener,
            url: url.to_string(),
            cancel,
        })
    }
    pub fn finish(self) -> Result<AccountIdentity> {
        let deadline = Instant::now() + Duration::from_secs(300);
        loop {
            ensure!(!self.cancel.is_cancelled(), "登录已取消");
            ensure!(Instant::now() < deadline, "登录已超时，请重试");
            let (mut stream, _) = match self.listener.accept() {
                Ok(v) => v,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    use std::os::fd::AsFd;
                    match zork_notify::io::readable(
                        self.listener.as_fd(),
                        None,
                        Some(&self.cancel),
                        deadline,
                    )? {
                        zork_notify::io::Ready::Readable => continue,
                        zork_notify::io::Ready::Cancelled => anyhow::bail!("登录已取消"),
                        _ => anyhow::bail!("登录已超时，请重试"),
                    }
                }
                Err(e) => return Err(e.into()),
            };
            stream.set_read_timeout(Some(Duration::from_secs(2)))?;
            let mut data = [0; 8192];
            let count = match stream.read(&mut data) {
                Ok(n) => n,
                Err(_) => continue,
            };
            let text = String::from_utf8_lossy(&data[..count]);
            let Some(path) = text
                .lines()
                .next()
                .and_then(|line| line.strip_prefix("GET "))
                .and_then(|line| line.split(' ').next())
            else {
                continue;
            };
            let Ok(callback) = reqwest::Url::parse(&format!("http://127.0.0.1{path}")) else {
                continue;
            };
            let pairs = callback.query_pairs().collect::<Vec<_>>();
            let states = pairs
                .iter()
                .filter(|(k, _)| k == "state")
                .collect::<Vec<_>>();
            if callback.path() != "/oauth/callback"
                || states.len() != 1
                || states[0].1.as_ref() != self.state.secret()
            {
                let _=stream.write_all(b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\nContent-Length: 22\r\n\r\nInvalid login callback");
                continue;
            }
            let codes = pairs
                .iter()
                .filter(|(k, _)| k == "code")
                .collect::<Vec<_>>();
            if codes.len() != 1 {
                let _=stream.write_all(b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\nContent-Length: 15\r\n\r\nLogin cancelled");
                anyhow::bail!("Cue 未批准登录，请重试")
            }
            let code = codes[0].1.to_string();
            let result = self.exchange(code);
            let body = if result.is_ok() {
                "Zork 登录完成，可以返回客户端。"
            } else {
                "Zork 登录未完成，请返回客户端查看提示。"
            };
            let response=format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nCache-Control: no-store\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",body.len());
            let _ = stream.write_all(response.as_bytes());
            return result;
        }
    }
    fn exchange(self, code: String) -> Result<AccountIdentity> {
        ensure!(!self.cancel.is_cancelled(), "登录已取消");
        let cancel = self.cancel.clone();
        let client = CoreClient::from_provider_metadata(
            self.metadata,
            ClientId::new(self.config.client_id.clone()),
            None,
        )
        .set_redirect_uri(RedirectUrl::new(self.redirect)?);
        let token = client
            .exchange_code(AuthorizationCode::new(code))?
            .set_pkce_verifier(self.verifier)
            .request(&http()?)
            .context("Cue 授权码交换失败")?;
        ensure!(!cancel.is_cancelled(), "登录已取消");
        let id_token = token.id_token().context("Cue 未返回身份凭据")?;
        let verifier = client.id_token_verifier();
        let claims = id_token
            .claims(&verifier, &self.nonce)
            .context("Cue 身份凭据校验失败")?;
        if let Some(expected) = claims.access_token_hash() {
            let actual = AccessTokenHash::from_token(
                token.access_token(),
                id_token.signing_alg()?,
                id_token.signing_key(&verifier)?,
            )?;
            ensure!(&actual == expected, "Cue token 校验失败");
        }
        // Cue's OIDC token currently only authenticates /userinfo. Do not use it
        // as a Relay, Mesh, or Cue product-API credential. Persist identity only.
        let email = claims.email().map(|v| v.as_str().to_owned());
        let name = claims
            .name()
            .and_then(|n| n.get(None))
            .map(|n| n.as_str().to_owned())
            .or_else(|| email.clone())
            .unwrap_or_else(|| claims.subject().as_str().to_owned());
        Ok(AccountIdentity {
            issuer: self.config.issuer,
            client_id: self.config.client_id,
            subject: claims.subject().as_str().into(),
            name,
            email,
            expires_at: claims.expiration().timestamp(),
        })
    }
}
fn http() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(15))
        .build()?)
}
