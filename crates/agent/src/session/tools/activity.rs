//! Tool-owned presentation metadata. Only explicitly selected targets leave the
//! tool boundary; argument objects, message bodies and typed text stay private.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ToolActivity {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub action: String,
    pub labels: BTreeMap<String, String>,
    pub detail: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<ActivityTarget>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum ActivityTarget {
    Agent(String),
    Task(String),
}

impl Default for ToolActivity {
    fn default() -> Self {
        Self::new("执行操作", "Working", "")
    }
}

impl ToolActivity {
    pub fn new(zh: &str, en: &str, detail: &str) -> Self {
        Self {
            action: String::new(),
            labels: [("zh-CN".into(), bounded(zh)), ("en".into(), bounded(en))].into(),
            detail: bounded(detail),
            target: None,
        }
    }

    /// RFC 6901 pointer, including nested tool arguments such as /action/url.
    pub fn field(zh: &str, en: &str, arguments: &Value, pointer: &str) -> Self {
        Self::new(
            zh,
            en,
            arguments
                .pointer(pointer)
                .and_then(Value::as_str)
                .unwrap_or_default(),
        )
    }

    pub fn target(mut self, target: ActivityTarget) -> Self {
        self.target = Some(target);
        self
    }
}

pub fn bounded(text: &str) -> String {
    let mut result = String::new();
    let mut space = false;
    for ch in text.chars().take(512) {
        if ch.is_whitespace() || ch.is_control() {
            space = !result.is_empty();
        } else {
            if space {
                result.push(' ');
                space = false;
            }
            result.push(ch);
        }
    }
    result
}
