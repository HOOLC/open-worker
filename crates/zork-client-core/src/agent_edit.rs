//! Core-owned Agent configuration choices and validation.
use crate::api::{ProfileInfo, ProfileModel};
use serde_json::Value;
pub fn thinking_after_choice(model: Option<&ProfileModel>, thinking: &str) -> String {
    model
        .map(|m| {
            if m.thinking.iter().any(|t| t == thinking) {
                thinking.into()
            } else {
                m.default_thinking.clone()
            }
        })
        .unwrap_or_else(|| "off".into())
}

pub fn compatible_profiles(
    profiles: &[ProfileInfo],
    model: Option<&str>,
    thinking: &str,
) -> Vec<usize> {
    let Some(model) = model else {
        return vec![0];
    };
    profiles
        .iter()
        .enumerate()
        .filter(|(_, p)| {
            p.profile_id == "auto"
                || p.models
                    .iter()
                    .any(|m| m.enabled && m.id == model && m.thinking.iter().any(|t| t == thinking))
        })
        .map(|(i, _)| i)
        .collect()
}
pub fn repair_profile(
    profiles: &[ProfileInfo],
    profile: usize,
    model: Option<&str>,
    thinking: &str,
) -> usize {
    if compatible_profiles(profiles, model, thinking).contains(&profile) {
        profile
    } else {
        0
    }
}
pub fn choices(profiles: &[ProfileInfo], profile: &str, model: &str, thinking: &str) -> Value {
    let options = profile_options(profiles);
    let models = &options[0].models;
    let selected = models.iter().find(|m| m.id == model);
    let thinking = selected
        .map(|m| {
            if m.thinking.iter().any(|v| v == thinking) {
                thinking.to_owned()
            } else {
                m.default_thinking.clone()
            }
        })
        .unwrap_or_default();
    let selected_index = options
        .iter()
        .position(|p| p.profile_id == profile)
        .unwrap_or(0);
    let profile_index = repair_profile(&options, selected_index, Some(model), &thinking);
    let eligible = compatible_profiles(&options, Some(model), &thinking);
    serde_json::json!({"models":models,"levels":selected.map(|m|&m.thinking),"thinking":thinking,
        "profiles":eligible.iter().map(|i|&options[*i]).collect::<Vec<_>>(),"profile":options[profile_index].profile_id,
        "valid":validate_selection(profiles,&options[profile_index].profile_id,model,&thinking).is_ok()})
}
pub fn profile_options(profiles: &[ProfileInfo]) -> Vec<ProfileInfo> {
    let mut options = profiles.to_vec();
    for profile in &mut options {
        profile.models.retain(|model| model.enabled);
    }
    let mut models: Vec<ProfileModel> = Vec::new();
    for model in options.iter().flat_map(|p| p.models.iter()) {
        if let Some(existing) = models.iter_mut().find(|m| m.id == model.id) {
            for effort in &model.thinking {
                if !existing.thinking.contains(effort) {
                    existing.thinking.push(effort.clone());
                }
            }
        } else {
            models.push(model.clone());
        }
    }
    options.insert(
        0,
        ProfileInfo {
            profile_id: "auto".into(),
            name: Some("自动分配".into()),
            provider: "auto".into(),
            billing: None,
            verified: false,
            account: Value::Null,
            rate_limits: Value::Null,
            checked_at: None,
            models,
            extra: Default::default(),
        },
    );
    options
}

pub fn validate_selection(
    profiles: &[ProfileInfo],
    profile: &str,
    model: &str,
    thinking: &str,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        profiles.iter().any(|p| (profile.is_empty()
            || profile == "auto"
            || p.profile_id == profile)
            && p.models
                .iter()
                .any(|m| m.enabled && m.id == model && m.thinking.iter().any(|t| t == thinking))),
        "请选择可用的模型、思考深度和模型连接"
    );
    Ok(())
}

use serde::Deserialize;
#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentInput {
    pub id: String,
    pub creating: bool,
    pub name: String,
    pub role: String,
    pub avatar: String,
    pub profile: String,
    pub model: String,
    pub thinking: String,
    pub instructions: String,
    pub allowed: Vec<String>,
}
