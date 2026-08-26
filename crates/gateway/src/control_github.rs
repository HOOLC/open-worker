use std::fs;

use anyhow::{Context, Result};
use serde_json::{json, Value};

use crate::config::{now_rfc3339, RuntimeConfig};

pub fn list_mappings(config: &RuntimeConfig) -> Result<Vec<Value>> {
    let dir = config.github_mappings_dir();
    fs::create_dir_all(&dir).ok();
    let mut mappings = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.filter_map(|entry| entry.ok()) {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            if let Ok(raw) = fs::read_to_string(&path) {
                if let Ok(value) = serde_json::from_str::<Value>(&raw) {
                    mappings.push(value);
                }
            }
        }
    }
    Ok(mappings)
}

pub fn upsert_mapping(
    config: &RuntimeConfig,
    slack_user_id: &str,
    github_author: &str,
) -> Result<Value> {
    if slack_user_id.is_empty() || slack_user_id.contains('/') {
        anyhow::bail!("invalid slack user id");
    }
    let dir = config.github_mappings_dir();
    fs::create_dir_all(&dir)?;
    let now = now_rfc3339();
    let record = json!({
        "platform": "slack",
        "userId": slack_user_id,
        "githubAuthor": github_author,
        "source": "manual",
        "updatedAt": now,
        "createdAt": now,
    });
    let path = dir.join(format!("slack-{slack_user_id}.json"));
    fs::write(path, serde_json::to_vec_pretty(&record)?)?;
    Ok(record)
}

pub fn delete_mapping(config: &RuntimeConfig, slack_user_id: &str) -> Result<()> {
    if slack_user_id.contains('/') {
        anyhow::bail!("invalid slack user id");
    }
    let path = config
        .github_mappings_dir()
        .join(format!("slack-{slack_user_id}.json"));
    if path.exists() {
        fs::remove_file(path).context("delete github mapping")?;
    }
    Ok(())
}
