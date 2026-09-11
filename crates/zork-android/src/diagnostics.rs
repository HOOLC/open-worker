//! Bounded transport-only diagnostics for debug builds. Never logs RPC bodies.
use std::{
    fs::File,
    io::{self, Write},
    path::Path,
    sync::Mutex,
};

struct LimitedLog {
    file: File,
    written: usize,
}
impl Write for LimitedLog {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        const LIMIT: usize = 2 * 1024 * 1024;
        let keep = bytes.len().min(LIMIT.saturating_sub(self.written));
        self.file.write_all(&bytes[..keep])?;
        self.written += keep;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

pub(super) fn init(root: &Path) {
    init_journal(root);
    let Ok(file) = File::create(root.join("transport-debug.log")) else {
        return;
    };
    let _ = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(Mutex::new(LimitedLog { file, written: 0 }))
        .with_env_filter("off,iroh::socket::transports=debug,iroh::socket::remote_map::remote_state=debug,iroh::endpoint=debug,iroh::protocol=debug,zork_mesh::node=debug,rustls_platform_verifier=warn")
        .try_init();
}

struct ErrorJournal {
    file: File,
    written: u64,
    last_state_error: Option<String>,
}
static JOURNAL: std::sync::OnceLock<Mutex<ErrorJournal>> = std::sync::OnceLock::new();
fn init_journal(root: &Path) {
    if JOURNAL.get().is_some() {
        return;
    }
    if let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("connection-errors.jsonl"))
    {
        let written = file.metadata().map(|m| m.len()).unwrap_or(0);
        let _ = JOURNAL.set(Mutex::new(ErrorJournal {
            file,
            written,
            last_state_error: None,
        }));
    }
}

// Preserve transient errors after the next snapshot clears the on-screen
// banner. This records no request bodies, messages, auth headers or tokens.
pub(super) fn record_response(
    request: &str,
    result: &anyhow::Result<serde_json::Value>,
    elapsed: std::time::Duration,
) {
    let Some(journal) = JOURNAL.get() else {
        return;
    };
    let Ok(mut journal) = journal.lock() else {
        return;
    };
    let op = serde_json::from_str::<serde_json::Value>(request)
        .ok()
        .and_then(|v| v.get("op").and_then(|v| v.as_str()).map(str::to_owned))
        .unwrap_or_default();
    let mut entry = match result {
        Err(error) => {
            Some(serde_json::json!({"kind":"command_error","error":format!("{error:#}")}))
        }
        Ok(data) => {
            if let Some(state) = data.get("state") {
                let error = state
                    .get("error")
                    .and_then(|v| v.as_str())
                    .filter(|v| !v.is_empty())
                    .map(str::to_owned);
                if error != journal.last_state_error {
                    journal.last_state_error = error.clone();
                    Some(
                        serde_json::json!({"kind":"state_error_changed","error":error,"connected":state.get("connected")}),
                    )
                } else {
                    None
                }
            } else {
                None
            }
        }
    };
    if entry.is_none()
        && matches!(
            op.as_str(),
            "resume" | "pause" | "subscribe" | "unsubscribe"
        )
    {
        entry = Some(serde_json::json!({"kind":"lifecycle","ok":result.is_ok()}));
    }
    let Some(mut entry) = entry else {
        return;
    };
    entry["op"] = op.into();
    entry["at_ms"] = serde_json::json!(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis());
    entry["elapsed_ms"] = serde_json::json!(elapsed.as_millis());
    if journal.written >= 512 * 1024 {
        return;
    }
    if let Ok(mut bytes) = serde_json::to_vec(&entry) {
        bytes.push(b'\n');
        if journal.file.write_all(&bytes).is_ok() {
            journal.written += bytes.len() as u64;
        }
    }
}
