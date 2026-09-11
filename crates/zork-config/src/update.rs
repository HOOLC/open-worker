//! Shared state for complete native node updates. Only installed background nodes
//! are eligible; application bundles and development checkouts are never modified.
use anyhow::{ensure, Result};
use serde_json::{json, Value};
use std::{fs, path::Path};

pub const RELEASE_BASE: &str = "https://github.com/HOOLC/open-worker/releases";

pub fn valid_version(version: &str) -> bool {
    if version.len() > 64 {
        return false;
    }
    let (core, suffix) = version.split_once('-').unwrap_or((version, ""));
    let parts: Vec<_> = core.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
        && (!version.contains('-') || !suffix.is_empty())
        && suffix
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'))
}

pub fn eligible(root: &Path, executable: &Path) -> Result<()> {
    ensure!(
        crate::service::settings(root)?.enabled,
        "请先将设备设为后台运行，再升级版本。"
    );
    ensure!(
        !fs::symlink_metadata(root.join("bin"))?
            .file_type()
            .is_symlink(),
        "此安装使用外部版本目录，请通过原安装方式升级。"
    );
    let bin = root.join("bin").canonicalize()?;
    ensure!(
        executable.canonicalize()?.parent() == Some(bin.as_path()),
        "此安装由客户端应用或开发环境管理，请更新对应应用。"
    );
    for name in ["zork", "zork-gateway", "zork-agent", "zork-gh"] {
        ensure!(bin.join(name).is_file(), "设备缺少完整版本包：{name}");
    }
    Ok(())
}

pub fn state(root: &Path) -> Value {
    let mut state = fs::read(root.join("run/update.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or(json!({"phase":"idle"}));
    if matches!(state["phase"].as_str(), Some("downloading" | "restarting"))
        && crate::service::exclusive_lock(&root.join("run/update.lock")).is_ok()
    {
        state["phase"] = json!("failed");
        state["message"] = json!("升级进程已中断，请检查设备状态后重试。");
    }
    state
}

pub fn write_state(root: &Path, phase: &str, version: &str, message: &str) -> Result<()> {
    // A fresh attempt at the same release must not reuse its old terminal state.
    let previous = fs::read(root.join("run/update.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    let operation = previous
        .as_ref()
        .filter(|value| phase != "downloading" && value["version"] == version)
        .and_then(|value| value["operation_id"].as_str())
        .map(str::to_owned)
        .unwrap_or_else(crate::random_token);
    let path = root.join("run/update.json.tmp");
    fs::write(
        &path,
        serde_json::to_vec(
            &json!({"phase":phase,"version":version,"message":message,"operation_id":operation}),
        )?,
    )?;
    fs::rename(path, root.join("run/update.json"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn release_versions_cannot_be_paths_or_arguments() {
        for v in ["0.1.30", "1.2.3-rc.1"] {
            assert!(valid_version(v));
        }
        for v in [
            "",
            "latest",
            "../1.2.3",
            "--help",
            "1.2",
            "1.2.3-",
            "1.2.3\n",
            "1.2.3/evil",
        ] {
            assert!(!valid_version(v), "{v}");
        }
    }

    #[test]
    fn unfinished_status_requires_a_live_update_lock() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("run")).unwrap();
        let lock = crate::service::exclusive_lock(&temp.path().join("run/update.lock")).unwrap();
        write_state(temp.path(), "downloading", "1.2.3", "Downloading").unwrap();
        let first = state(temp.path())["operation_id"].clone();
        assert!(first.as_str().is_some_and(|id| !id.is_empty()));
        assert_eq!(state(temp.path())["phase"], "downloading");
        drop(lock);
        assert_eq!(state(temp.path())["phase"], "failed");
        write_state(temp.path(), "complete", "1.2.3", "Done").unwrap();
        assert_eq!(state(temp.path())["phase"], "complete");
        assert_eq!(state(temp.path())["operation_id"], first);
        write_state(temp.path(), "downloading", "1.2.3", "Again").unwrap();
        assert_ne!(state(temp.path())["operation_id"], first);
    }
}
