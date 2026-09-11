//! Structured requests and authoritative results carried by ordinary Chat messages.
//! Deserializing a message never executes its action. Client cache projections may
//! attach a resolution to the original request without modifying the source.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};

pub const VERSION: u32 = 1;
pub const MAX_FIELDS: usize = 16;
pub const MAX_INPUT_BYTES: usize = 32 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub profile_id: String,
    pub model: String,
    pub thinking: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentConfig {
    pub name: String,
    pub selection: Selection,
    #[serde(default)]
    pub avatar: Option<String>,
    #[serde(default)]
    pub instructions: String,
    #[serde(default)]
    pub skill_paths: Vec<String>,
    #[serde(default)]
    pub allowed_leaders: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Choice {
    pub value: String,
    pub label: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldKind {
    #[default]
    Text,
    Multiline,
    Choice,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Field {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub kind: FieldKind,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: String,
    #[serde(default)]
    pub options: Vec<Choice>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", deny_unknown_fields)]
pub enum Request {
    #[serde(rename = "agent.create")]
    CreateAgent { config: AgentConfig },
    #[serde(rename = "agent.update")]
    UpdateAgent {
        agent_id: String,
        expected_revision: String,
        config: AgentConfig,
    },
    #[serde(rename = "input")]
    Input { title: String, fields: Vec<Field> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Completed,
    Declined,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resolution {
    pub request_message_id: String,
    pub response_id: String,
    pub revision: u64,
    pub outcome: Outcome,
    pub actor: String,
    pub output: Value,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Content {
    Request { request: Request },
    Result { result: Resolution },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageContent {
    pub version: u32,
    #[serde(flatten)]
    pub content: Content,
}

impl MessageContent {
    pub fn request(request: Request) -> Self {
        Self {
            version: VERSION,
            content: Content::Request { request },
        }
    }
    pub fn result(result: Resolution) -> Self {
        Self {
            version: VERSION,
            content: Content::Result { result },
        }
    }
    /// Keep unknown versions in the enclosing message's raw JSON so older
    /// clients can show its text, without guessing an executable operation.
    pub fn parse(value: &Value) -> Option<Self> {
        let content: Self = serde_json::from_value(value.clone()).ok()?;
        (content.version == VERSION).then_some(content)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub response_id: String,
    pub accept: bool,
    #[serde(default)]
    pub values: BTreeMap<String, String>,
}

pub fn valid_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 512 && !value.chars().any(char::is_control)
}

impl AgentConfig {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.name.trim().is_empty()
            || self.name.len() > 160
            || self.name.chars().any(char::is_control)
        {
            return Err("invalid_agent_name");
        }
        if !valid_id(&self.selection.profile_id)
            || !valid_id(&self.selection.model)
            || !valid_id(&self.selection.thinking)
            || self.instructions.len() > MAX_INPUT_BYTES
        {
            return Err("invalid_agent_configuration");
        }
        if self.skill_paths.len() > 32
            || self.allowed_leaders.len() > 64
            || self
                .skill_paths
                .iter()
                .chain(&self.allowed_leaders)
                .any(|s| !valid_id(s))
        {
            return Err("invalid_agent_resources");
        }
        Ok(())
    }
}

impl Request {
    pub fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::CreateAgent { config } => config.validate(),
            Self::UpdateAgent {
                agent_id,
                expected_revision,
                config,
            } => {
                if !valid_id(agent_id) || !valid_id(expected_revision) {
                    return Err("invalid_agent_reference");
                }
                config.validate()
            }
            Self::Input { title, fields } => {
                if title.trim().is_empty()
                    || title.len() > 512
                    || fields.is_empty()
                    || fields.len() > MAX_FIELDS
                    || fields.iter().map(|f| f.default.len()).sum::<usize>() > MAX_INPUT_BYTES
                {
                    return Err("invalid_input_request");
                }
                let mut ids = HashSet::new();
                for field in fields {
                    if field.id.is_empty()
                        || field.id.len() > 64
                        || !field
                            .id
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                        || !ids.insert(&field.id)
                        || field.label.trim().is_empty()
                        || field.label.len() > 512
                        || field.default.len() > 8192
                        || field.options.len() > 32
                    {
                        return Err("invalid_input_field");
                    }
                    let mut options = HashSet::new();
                    if field.options.iter().any(|c| {
                        !valid_id(&c.value) || !valid_id(&c.label) || !options.insert(&c.value)
                    }) {
                        return Err("invalid_input_choices");
                    }
                    if field.kind == FieldKind::Choice {
                        if field.options.is_empty()
                            || (!field.default.is_empty() && !options.contains(&field.default))
                        {
                            return Err("invalid_input_choices");
                        }
                    } else if !field.options.is_empty() {
                        return Err("unexpected_input_choices");
                    }
                }
                Ok(())
            }
        }
    }

    /// The only editable Agent fields on a confirmation card are its name and
    /// instructions. Model, resources and grants remain explicitly visible and
    /// fixed by the proposal; a different configuration is a new proposal.
    pub fn defaults(&self) -> BTreeMap<String, String> {
        match self {
            Self::CreateAgent { config } | Self::UpdateAgent { config, .. } => [
                ("name".into(), config.name.clone()),
                ("instructions".into(), config.instructions.clone()),
            ]
            .into(),
            Self::Input { fields, .. } => fields
                .iter()
                .map(|f| (f.id.clone(), f.default.clone()))
                .collect(),
        }
    }

    pub fn validate_values(
        &self,
        values: &BTreeMap<String, String>,
    ) -> Result<BTreeMap<String, String>, BTreeMap<String, String>> {
        let mut merged = self.defaults();
        let mut errors = BTreeMap::new();
        if values.values().map(String::len).sum::<usize>() > MAX_INPUT_BYTES {
            errors.insert(String::new(), "input_too_large".into());
        }
        for (id, value) in values {
            if !merged.contains_key(id) {
                errors.insert(id.clone(), "unknown_input_field".into());
            } else {
                merged.insert(id.clone(), value.clone());
            }
        }
        if merged.values().map(String::len).sum::<usize>() > MAX_INPUT_BYTES {
            errors.insert(String::new(), "input_too_large".into());
        }
        match self {
            Self::CreateAgent { config } | Self::UpdateAgent { config, .. } => {
                let mut config = config.clone();
                config.name = merged["name"].clone();
                config.instructions = merged["instructions"].clone();
                if let Err(error) = config.validate() {
                    errors.insert(
                        if error == "invalid_agent_name" {
                            "name"
                        } else {
                            "instructions"
                        }
                        .into(),
                        error.into(),
                    );
                }
            }
            Self::Input { fields, .. } => {
                for field in fields {
                    let value = &merged[&field.id];
                    let error = if field.required && value.trim().is_empty() {
                        Some("input_required")
                    } else if value.len() > 8192 {
                        Some("input_too_large")
                    } else if field.kind == FieldKind::Choice
                        && !value.is_empty()
                        && !field.options.iter().any(|c| c.value == *value)
                    {
                        Some("invalid_input_choice")
                    } else {
                        None
                    };
                    if let Some(error) = error {
                        errors.insert(field.id.clone(), error.into());
                    }
                }
            }
        }
        if errors.is_empty() {
            Ok(merged)
        } else {
            Err(errors)
        }
    }
}

/// One schema is shared by the tool catalog and its gateway validation.
pub fn request_schema() -> Value {
    let string = || json!({"type":"string","minLength":1,"maxLength":512});
    let config = json!({"type":"object","additionalProperties":false,"required":["name","selection"],"properties":{
        "name":{"type":"string","minLength":1,"maxLength":160},
        "selection":{"type":"object","additionalProperties":false,"required":["profile_id","model","thinking"],"properties":{"profile_id":string(),"model":string(),"thinking":string()}},
        "avatar":{"type":["string","null"]},"instructions":{"type":"string","maxLength":32768},
        "skill_paths":{"type":"array","maxItems":32,"items":string()},"allowed_leaders":{"type":"array","maxItems":64,"uniqueItems":true,"items":string()}
    }});
    let field = json!({"type":"object","additionalProperties":false,"required":["id","label"],"properties":{
        "id":{"type":"string","pattern":"^[a-zA-Z0-9_-]{1,64}$"},"label":string(),"kind":{"enum":["text","multiline","choice"]},"required":{"type":"boolean"},"default":{"type":"string","maxLength":8192},
        "options":{"type":"array","maxItems":32,"items":{"type":"object","additionalProperties":false,"required":["value","label"],"properties":{"value":string(),"label":string()}}}
    }});
    json!({"oneOf":[
        {"type":"object","additionalProperties":false,"required":["action","config"],"properties":{"action":{"const":"agent.create"},"config":config}},
        {"type":"object","additionalProperties":false,"required":["action","agent_id","expected_revision","config"],"properties":{"action":{"const":"agent.update"},"agent_id":string(),"expected_revision":string(),"config":config}},
        {"type":"object","additionalProperties":false,"required":["action","title","fields"],"properties":{"action":{"const":"input"},"title":string(),"fields":{"type":"array","minItems":1,"maxItems":MAX_FIELDS,"items":field}}}
    ]})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> Request {
        Request::Input {
            title: "Choose a target".into(),
            fields: vec![Field {
                id: "target".into(),
                label: "Target".into(),
                kind: FieldKind::Choice,
                required: true,
                default: "test".into(),
                options: vec![Choice {
                    value: "test".into(),
                    label: "Test".into(),
                }],
            }],
        }
    }

    #[test]
    fn inputs_validate_defaults_choices_and_unknown_fields() {
        let request = input();
        request.validate().unwrap();
        assert_eq!(
            request.validate_values(&BTreeMap::new()).unwrap()["target"],
            "test"
        );
        assert!(request
            .validate_values(&[("target".into(), "production".into())].into())
            .is_err());
        assert!(request
            .validate_values(&[("target".into(), "".into())].into())
            .is_err());
        assert!(request
            .validate_values(&[("permission".into(), "admin".into())].into())
            .is_err());
    }

    #[test]
    fn unknown_versions_are_not_executable_and_duplicate_fields_are_rejected() {
        let mut value = serde_json::to_value(MessageContent::request(input())).unwrap();
        assert!(MessageContent::parse(&value).is_some());
        value["version"] = json!(999);
        assert!(MessageContent::parse(&value).is_none());
        let Request::Input { mut fields, title } = input() else {
            unreachable!()
        };
        fields.push(fields[0].clone());
        assert!(Request::Input { title, fields }.validate().is_err());
    }
}
