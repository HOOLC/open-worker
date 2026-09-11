//! Selection metadata only; skill bodies remain ordinary files.
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    path::{Component, Path, PathBuf},
};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct BundleState {
    pub active: Option<BundleSelection>,
    pub disabled: BTreeSet<String>,
    pub distribution_revision: Option<String>,
    pub rollback: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BundleSelection {
    pub directory: String,
    pub skills: Vec<String>,
}
pub fn valid_component(value: &str) -> bool {
    !value.is_empty()
        && !value.contains(['/', '\\'])
        && !value.chars().any(char::is_control)
        && !value.starts_with('.')
        && Path::new(value).components().count() == 1
        && matches!(
            Path::new(value).components().next(),
            Some(Component::Normal(_))
        )
}
pub fn load(root: &Path) -> Result<BundleState> {
    let path = root.join(".state.json");
    let state: BundleState = match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => BundleState::default(),
        Err(e) => return Err(e.into()),
    };
    if let Some(active) = &state.active {
        ensure!(
            valid_component(&active.directory) && active.skills.iter().all(|s| valid_component(s)),
            "invalid bundled skill selection"
        );
    }
    Ok(state)
}
pub fn sources(data_root: &Path) -> Result<Vec<PathBuf>> {
    let root = data_root.join("bundled-skills");
    let state = load(&root)?;
    Ok(match state.active {
        Some(active) => active
            .skills
            .iter()
            .filter(|s| !state.disabled.contains(*s))
            .map(|s| root.join(".versions").join(&active.directory).join(s))
            .collect(),
        None => Vec::new(),
    })
}
