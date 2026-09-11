//! Bounded MCP 2025-11-25 client. Connections belong to one execution subject.
use anyhow::{bail, ensure, Context, Result};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::BTreeMap, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout},
};

pub const MAX_MESSAGE: usize = 8 * 1024 * 1024;
const VERSION: &str = "2025-11-25";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Transport {
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        cwd: String,
        #[serde(default)]
        env: BTreeMap<String, String>,
        #[serde(default)]
        secret_env: BTreeMap<String, String>,
    },
    Http {
        url: String,
        #[serde(default)]
        secret_headers: BTreeMap<String, String>,
    },
}
impl Transport {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Stdio {
                command,
                cwd,
                args,
                env,
                secret_env,
            } => {
                ensure!(
                    std::path::Path::new(command).is_absolute()
                        && std::path::Path::new(cwd).is_absolute(),
                    "mcp_absolute_command_and_cwd_required"
                );
                ensure!(
                    args.len() <= 128 && env.len() + secret_env.len() <= 64,
                    "mcp_config_limit"
                );
            }
            Self::Http {
                url,
                secret_headers,
            } => {
                let u = reqwest::Url::parse(url)?;
                ensure!(
                    u.username().is_empty() && u.password().is_none() && u.fragment().is_none(),
                    "mcp_invalid_endpoint"
                );
                ensure!(
                    u.scheme() == "https"
                        || (u.scheme() == "http"
                            && matches!(u.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))),
                    "mcp_https_required"
                );
                for key in secret_headers.keys() {
                    ensure!(
                        !matches!(
                            key.to_ascii_lowercase().as_str(),
                            "host"
                                | "content-type"
                                | "content-length"
                                | "accept"
                                | "mcp-session-id"
                                | "mcp-protocol-version"
                        ),
                        "mcp_reserved_header"
                    );
                }
            }
        }
        Ok(())
    }
}

pub struct Client {
    transport: Connection,
    next_id: u64,
    pub tools: bool,
}
pub type Cleanup = tokio::sync::watch::Receiver<Option<bool>>;
struct OwnedChild {
    child: Option<Child>,
    stopped: tokio::sync::watch::Sender<Option<bool>>,
}
impl OwnedChild {
    fn new(child: Child) -> Self {
        Self {
            child: Some(child),
            stopped: tokio::sync::watch::channel(None).0,
        }
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        #[cfg(unix)]
        if let Some(pid) = child.id() {
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
        }
        let _ = child.start_kill();
        let stopped = self.stopped.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                stopped.send_replace(Some(child.wait().await.is_ok()));
            });
        } else {
            stopped.send_replace(Some(false));
        }
    }
}
pub async fn await_cleanup(mut cleanup: Cleanup) -> Result<()> {
    loop {
        if let Some(confirmed) = *cleanup.borrow_and_update() {
            ensure!(confirmed, "mcp_termination_unconfirmed");
            return Ok(());
        }
        cleanup
            .changed()
            .await
            .context("mcp_termination_unconfirmed")?;
    }
}
enum Connection {
    Stdio {
        _child: OwnedChild,
        input: ChildStdin,
        output: BufReader<ChildStdout>,
    },
    Http {
        client: reqwest::Client,
        url: String,
        headers: reqwest::header::HeaderMap,
        session: Option<String>,
    },
}
impl Drop for Client {
    fn drop(&mut self) {
        if let Connection::Http {
            client,
            url,
            headers,
            session: Some(session),
        } = &self.transport
        {
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                let request = client
                    .delete(url)
                    .headers(headers.clone())
                    .header("MCP-Protocol-Version", VERSION)
                    .header("MCP-Session-Id", session);
                handle.spawn(async move {
                    let _ = tokio::time::timeout(Duration::from_secs(2), request.send()).await;
                });
            }
        }
    }
}
impl Client {
    pub async fn connect(config: &Transport) -> Result<Self> {
        Self::connect_tracked(config, &mut None).await
    }
    pub fn cleanup(&self) -> Option<Cleanup> {
        match &self.transport {
            Connection::Stdio { _child, .. } => Some(_child.stopped.subscribe()),
            _ => None,
        }
    }
    pub async fn connect_tracked(
        config: &Transport,
        cleanup: &mut Option<Cleanup>,
    ) -> Result<Self> {
        config.validate()?;
        let transport = match config {
            Transport::Stdio {
                command,
                args,
                cwd,
                env,
                secret_env,
            } => {
                let mut process = tokio::process::Command::new(command);
                process
                    .args(args)
                    .current_dir(cwd)
                    .env_clear()
                    .envs(env)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .kill_on_drop(true);
                #[cfg(unix)]
                process.process_group(0);
                for (key, reference) in secret_env {
                    process.env(
                        key,
                        std::env::var(reference)
                            .map_err(|_| anyhow::anyhow!("mcp_auth_required"))?,
                    );
                }
                let mut child = process
                    .spawn()
                    .map_err(|_| anyhow::anyhow!("mcp_spawn_failed"))?;
                Connection::Stdio {
                    input: child.stdin.take().context("mcp_stdin")?,
                    output: BufReader::new(child.stdout.take().context("mcp_stdout")?),
                    _child: OwnedChild::new(child),
                }
            }
            Transport::Http {
                url,
                secret_headers,
            } => {
                let mut headers = reqwest::header::HeaderMap::new();
                for (key, reference) in secret_headers {
                    headers.insert(
                        reqwest::header::HeaderName::from_bytes(key.as_bytes())?,
                        std::env::var(reference)
                            .map_err(|_| anyhow::anyhow!("mcp_auth_required"))?
                            .parse()?,
                    );
                }
                Connection::Http {
                    client: reqwest::Client::builder()
                        .redirect(reqwest::redirect::Policy::none())
                        .connect_timeout(Duration::from_secs(5))
                        .build()?,
                    url: url.clone(),
                    headers,
                    session: None,
                }
            }
        };
        let mut client = Self {
            transport,
            next_id: 1,
            tools: false,
        };
        *cleanup = client.cleanup();
        let hello = client.request("initialize", json!({"protocolVersion":VERSION,"capabilities":{},"clientInfo":{"name":"zork","version":"1"}})).await?;
        ensure!(
            hello["protocolVersion"] == VERSION,
            "mcp_unsupported_protocol_version"
        );
        client.tools = hello.pointer("/capabilities/tools").is_some();
        client
            .notify("notifications/initialized", json!({}))
            .await?;
        Ok(client)
    }
    pub async fn list_tools(&mut self) -> Result<Vec<Value>> {
        ensure!(self.tools, "mcp_tools_not_supported");
        let mut all = Vec::new();
        let mut cursor: Option<String> = None;
        let mut seen = std::collections::HashSet::new();
        let mut bytes = 0;
        loop {
            let result = self
                .request(
                    "tools/list",
                    cursor
                        .as_ref()
                        .map(|c| json!({"cursor":c}))
                        .unwrap_or(json!({})),
                )
                .await?;
            let tools = result["tools"].as_array().context("mcp_invalid_tools")?;
            for tool in tools {
                ensure!(
                    tool["name"].as_str().is_some_and(|n| !n.is_empty())
                        && tool["inputSchema"].is_object(),
                    "mcp_invalid_tool_definition"
                );
                ensure!(
                    !all.iter().any(|t: &Value| t["name"] == tool["name"]),
                    "mcp_duplicate_tool"
                );
                bytes += serde_json::to_vec(tool)?.len();
                ensure!(
                    bytes <= MAX_MESSAGE && all.len() < 1024,
                    "mcp_catalog_limit"
                );
                all.push(tool.clone());
            }
            cursor = result
                .get("nextCursor")
                .map(|c| c.as_str().map(str::to_owned).context("mcp_invalid_cursor"))
                .transpose()?;
            match &cursor {
                Some(c) => ensure!(
                    seen.insert(c.clone()) && seen.len() <= 128,
                    "mcp_cursor_cycle"
                ),
                None => break,
            }
        }
        Ok(all)
    }
    pub async fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let message = json!({"jsonrpc":"2.0","id":id,"method":method,"params":params});
        match &mut self.transport {
            Connection::Stdio { input, output, .. } => {
                write(input, &message).await?;
                for _ in 0..1024 {
                    let response = read(output).await?;
                    if response.get("method").is_some() {
                        if response.get("id").is_some() {
                            write(input, &server_reply(&response)).await?;
                        }
                        continue;
                    }
                    return result(response, id);
                }
                bail!("mcp_notification_limit")
            }
            Connection::Http {
                client,
                url,
                headers,
                session,
            } => {
                let mut request = client
                    .post(url.as_str())
                    .headers(headers.clone())
                    .header("Accept", "application/json, text/event-stream")
                    .json(&message);
                if method != "initialize" {
                    request = request.header("MCP-Protocol-Version", VERSION);
                }
                if let Some(s) = &session {
                    request = request.header("MCP-Session-Id", s);
                }
                let response = request
                    .send()
                    .await
                    .map_err(|_| anyhow::anyhow!("mcp_transport_failed"))?;
                ensure!(
                    !matches!(response.status().as_u16(), 401 | 403),
                    "mcp_auth_required"
                );
                ensure!(response.status().is_success(), "mcp_http_error");
                if method == "initialize" {
                    *session = response
                        .headers()
                        .get("MCP-Session-Id")
                        .map(|v| v.to_str().map(str::to_owned))
                        .transpose()?;
                }
                let sse = response
                    .headers()
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .is_some_and(|s| s.starts_with("text/event-stream"));
                let mut bytes = Vec::new();
                let mut stream = response.bytes_stream();
                let mut total = 0;
                while let Some(chunk) = stream.next().await {
                    let chunk = chunk.map_err(|_| anyhow::anyhow!("mcp_transport_failed"))?;
                    total += chunk.len();
                    ensure!(total <= MAX_MESSAGE, "mcp_response_limit");
                    bytes.extend_from_slice(&chunk);
                    if sse {
                        while let Some((end, length)) = event_end(&bytes) {
                            let event = String::from_utf8(bytes.drain(..end + length).collect())?;
                            let data = event
                                .lines()
                                .filter_map(|l| {
                                    l.strip_prefix("data:")
                                        .map(|d| d.strip_prefix(' ').unwrap_or(d))
                                })
                                .collect::<Vec<_>>()
                                .join("\n");
                            if data.is_empty() {
                                continue;
                            }
                            let value: Value = serde_json::from_str(&data)?;
                            if value.get("method").is_some() {
                                if value.get("id").is_some() {
                                    let mut reply = client
                                        .post(url.as_str())
                                        .headers(headers.clone())
                                        .header("Accept", "application/json, text/event-stream")
                                        .header("MCP-Protocol-Version", VERSION)
                                        .json(&server_reply(&value));
                                    if let Some(s) = &session {
                                        reply = reply.header("MCP-Session-Id", s);
                                    }
                                    let _ = reply.send().await?;
                                }
                                continue;
                            }
                            return result(value, id);
                        }
                    }
                }
                ensure!(!sse, "mcp_response_missing");
                result(serde_json::from_slice(&bytes)?, id)
            }
        }
    }
    async fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        let message = json!({"jsonrpc":"2.0","method":method,"params":params});
        match &mut self.transport {
            Connection::Stdio { input, .. } => write(input, &message).await,
            Connection::Http {
                client,
                url,
                headers,
                session,
            } => {
                let mut request = client
                    .post(url.as_str())
                    .headers(headers.clone())
                    .header("Accept", "application/json, text/event-stream")
                    .header("MCP-Protocol-Version", VERSION)
                    .json(&message);
                if let Some(s) = session {
                    request = request.header("MCP-Session-Id", s.as_str());
                }
                ensure!(
                    request.send().await?.status().is_success(),
                    "mcp_notification_failed"
                );
                Ok(())
            }
        }
    }
}
fn result(value: Value, id: u64) -> Result<Value> {
    ensure!(
        value["jsonrpc"] == "2.0" && value["id"] == id,
        "mcp_response_id_mismatch"
    );
    ensure!(value.get("error").is_none(), "mcp_protocol_error");
    value
        .get("result")
        .cloned()
        .context("mcp_response_missing_result")
}
async fn write(input: &mut ChildStdin, value: &Value) -> Result<()> {
    let mut bytes = serde_json::to_vec(value)?;
    ensure!(bytes.len() <= MAX_MESSAGE, "mcp_request_limit");
    bytes.push(b'\n');
    input.write_all(&bytes).await?;
    input.flush().await?;
    Ok(())
}
async fn read(output: &mut BufReader<ChildStdout>) -> Result<Value> {
    let mut bytes = Vec::new();
    loop {
        let buffer = output.fill_buf().await?;
        ensure!(!buffer.is_empty(), "mcp_process_closed");
        let end = buffer.iter().position(|b| *b == b'\n').map(|n| n + 1);
        let n = end.unwrap_or(buffer.len());
        ensure!(bytes.len() + n <= MAX_MESSAGE, "mcp_response_limit");
        bytes.extend_from_slice(&buffer[..n]);
        output.consume(n);
        if end.is_some() {
            return Ok(serde_json::from_slice(&bytes)?);
        }
    }
}
fn event_end(bytes: &[u8]) -> Option<(usize, usize)> {
    (0..bytes.len()).find_map(|i| {
        if bytes[i..].starts_with(b"\n\n") {
            Some((i, 2))
        } else if bytes[i..].starts_with(b"\r\n\r\n") {
            Some((i, 4))
        } else {
            None
        }
    })
}

fn server_reply(request: &Value) -> Value {
    if request["method"] == "ping" {
        json!({"jsonrpc":"2.0","id":request["id"],"result":{}})
    } else {
        json!({"jsonrpc":"2.0","id":request["id"],"error":{"code":-32601,"message":"Client capability not supported"}})
    }
}
