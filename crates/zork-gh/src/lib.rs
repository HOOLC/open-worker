use anyhow::{bail, Context, Result};
use reqwest::blocking::Client;
use reqwest::header::CONTENT_TYPE;
use reqwest::Url;
use serde_json::{json, Value};
use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;
const FETCH_TIMEOUT: Duration = Duration::from_secs(30);
pub fn zork_gh_main(argv: &[String]) -> Result<i32> {
    let broker_api_base = read_env("BROKER_API_BASE");
    let real_gh_path = read_env("BROKER_REAL_GH_PATH")
        .map(PathBuf::from)
        .or_else(find_real_gh);
    if broker_api_base.is_none() || real_gh_path.is_none() {
        bail!("BROKER_API_BASE and BROKER_REAL_GH_PATH are required for broker gh wrapper.");
    }
    let broker_api_base = broker_api_base.unwrap();
    let real_gh_path = real_gh_path.unwrap();
    let cwd = env::current_dir().context("cwd")?;
    match resolve_github_token(&broker_api_base, &cwd, argv)? {
        Ok(token) => run_real_gh(&real_gh_path, &cwd, argv, &token),
        Err(message) => {
            eprint!("{message}");
            if !message.ends_with('\n') {
                eprintln!();
            }
            Ok(1)
        }
    }
}

fn http_client() -> Result<Client> {
    Client::builder()
        .timeout(FETCH_TIMEOUT)
        .http1_only()
        .no_proxy()
        .build()
        .context("http client")
}

fn resolve_github_token(
    broker_api_base: &str,
    cwd: &Path,
    argv: &[String],
) -> Result<std::result::Result<String, String>> {
    let url = Url::parse(&format!(
        "{}/github-token/resolve",
        broker_api_base.trim_end_matches('/')
    ))
    .context("BROKER_API_BASE")?;
    let response = http_client()?
        .post(url)
        .header(CONTENT_TYPE, "application/json")
        .json(&json!({
            "cwd": cwd,
            "command": argv,
        }))
        .send()
        .context("github token resolve")?;
    let status = response.status().as_u16();
    let text = response.text().unwrap_or_default();
    let body: Value = if text.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str(&text).unwrap_or_else(|_| json!({}))
    };
    if !(200..300).contains(&status) || body.get("ok") != Some(&Value::Bool(true)) {
        return Ok(Err(token_error_message(status, &text)));
    }
    let token = body
        .get("token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    match token {
        Some(token) => Ok(Ok(token.to_string())),
        None => Ok(Err(
            "GitHub identity resolution did not return a token.".into()
        )),
    }
}

fn token_error_message(status: u16, text: &str) -> String {
    let body: Value = serde_json::from_str(text).unwrap_or_else(|_| json!({}));
    if let Some(message) = body.get("message").and_then(Value::as_str) {
        return format!("{message}\n");
    }
    if let Some(error) = body.get("error").and_then(Value::as_str) {
        return format!("{error}\n");
    }
    format!("GitHub identity resolution failed ({status}).\n")
}

fn run_real_gh(real_gh_path: &Path, cwd: &Path, argv: &[String], token: &str) -> Result<i32> {
    let mut command = Command::new(real_gh_path);
    command
        .args(argv)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    for key in ["GH_TOKEN", "GITHUB_TOKEN", "BROKER_DEFAULT_GITHUB_TOKEN"] {
        command.env_remove(key);
    }
    command.env("GH_TOKEN", token);
    let status = command
        .status()
        .with_context(|| format!("exec {}", real_gh_path.display()))?;
    Ok(status.code().unwrap_or(1))
}

fn find_real_gh() -> Option<PathBuf> {
    let skip = env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf));
    let skip_canon = skip.as_ref().and_then(|path| path.canonicalize().ok());
    let path_value = env::var_os("PATH")?;
    for dir in env::split_paths(&path_value) {
        if skip_canon
            .as_ref()
            .zip(dir.canonicalize().ok().as_ref())
            .is_some_and(|(skip, current)| skip == current)
        {
            continue;
        }
        let candidate = dir.join("gh");
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn is_executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .map(|meta| meta.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn read_env(key: &str) -> Option<String> {
    env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
