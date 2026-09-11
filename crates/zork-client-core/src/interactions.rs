//! Message interaction contracts and core-owned presentation/operation logic.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
pub use zork_client_types::interaction::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Submission {
    pub response: Response,
    #[serde(default)]
    pub attempted: bool,
    #[serde(default)]
    pub accepted: bool,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Activate {
        message_id: String,
        choice: String,
        #[serde(default)]
        values: BTreeMap<String, String>,
    },
    Submit {
        message_id: String,
        values: BTreeMap<String, String>,
    },
    Decline {
        message_id: String,
    },
    Retry {
        message_id: String,
    },
}
impl Command {
    pub fn from_action(
        message_id: &str,
        action: &str,
        values: BTreeMap<String, String>,
    ) -> anyhow::Result<Self> {
        match action {
            "submit" => Ok(Self::Submit {
                message_id: message_id.into(),
                values,
            }),
            "decline" => Ok(Self::Decline {
                message_id: message_id.into(),
            }),
            "retry" => Ok(Self::Retry {
                message_id: message_id.into(),
            }),
            _ => anyhow::bail!("Unsupported interaction action"),
        }
    }
    pub fn message_id(&self) -> &str {
        match self {
            Self::Activate { message_id, .. }
            | Self::Submit { message_id, .. }
            | Self::Decline { message_id }
            | Self::Retry { message_id } => message_id,
        }
    }
    pub(crate) fn resolve(self) -> anyhow::Result<Self> {
        match self {
            Self::Activate {
                message_id,
                choice,
                values,
            } => Self::from_action(&message_id, &choice, values),
            command => Ok(command),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardField {
    pub field: Field,
    pub localized_label: bool,
    pub value: String,
    pub error_key: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Detail {
    pub label_key: String,
    pub value: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardAction {
    pub id: String,
    pub label_key: String,
    pub primary: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Card {
    pub message_id: String,
    pub title: String,
    pub localized_title: bool,
    pub status_key: String,
    pub fields: Vec<CardField>,
    pub details: Vec<Detail>,
    pub actions: Vec<CardAction>,
    pub editable: bool,
    pub error: Option<String>,
}

/// Portable read-only presentation, also used by deterministic Web fixtures.
pub fn card(
    message_id: &str,
    request: &Request,
    resolution: Option<&Resolution>,
    submission: Option<&Submission>,
    errors: &BTreeMap<String, String>,
) -> Card {
    let (title, localized_title, submit, fields, details) = match request {
        Request::CreateAgent { config } | Request::UpdateAgent { config, .. } => {
            let updating = matches!(request, Request::UpdateAgent { .. });
            let fields = vec![
                Field {
                    id: "name".into(),
                    label: "interaction_name".into(),
                    kind: FieldKind::Text,
                    required: true,
                    default: config.name.clone(),
                    options: vec![],
                },
                Field {
                    id: "instructions".into(),
                    label: "interaction_instructions".into(),
                    kind: FieldKind::Multiline,
                    required: false,
                    default: config.instructions.clone(),
                    options: vec![],
                },
            ];
            let mut details = vec![Detail {
                label_key: "interaction_model".into(),
                value: format!(
                    "{} · {} · {}",
                    config.selection.profile_id, config.selection.model, config.selection.thinking
                ),
            }];
            if !config.skill_paths.is_empty() {
                details.push(Detail {
                    label_key: "interaction_skills".into(),
                    value: config.skill_paths.join("\n"),
                });
            }
            if !config.allowed_leaders.is_empty() {
                details.push(Detail {
                    label_key: "interaction_grants".into(),
                    value: config.allowed_leaders.join("\n"),
                });
            }
            (
                (if updating {
                    "interaction_update_agent"
                } else {
                    "interaction_create_agent"
                })
                .into(),
                true,
                if updating {
                    "interaction_confirm_update"
                } else {
                    "interaction_confirm_create"
                },
                fields,
                details,
            )
        }
        Request::Input { title, fields } => (
            title.clone(),
            false,
            "interaction_submit",
            fields.clone(),
            vec![],
        ),
    };
    let mut values = request.defaults();
    if let Some(submission) = submission {
        values.extend(submission.response.values.clone());
    }
    if let Some(resolution) = resolution {
        if let Some(actual) = resolution
            .output
            .get("values")
            .and_then(serde_json::Value::as_object)
        {
            values.extend(
                actual
                    .iter()
                    .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned()))),
            );
        }
        if let Some(agent) = resolution.output.get("agent") {
            for key in ["name", "instructions"] {
                if let Some(value) = agent[key].as_str() {
                    values.insert(key.into(), value.into());
                }
            }
        }
    }
    let ready = resolution.is_none() && submission.is_none();
    let retry = resolution.is_none() && submission.is_some_and(|s| s.error.is_some());
    let status = if let Some(result) = resolution {
        if result.outcome == Outcome::Completed {
            "interaction_completed"
        } else {
            "interaction_declined"
        }
    } else if retry {
        "interaction_unconfirmed"
    } else if submission.is_some() {
        "interaction_submitting"
    } else {
        "interaction_confirmation"
    };
    let actions = if ready {
        vec![
            CardAction {
                id: "submit".into(),
                label_key: submit.into(),
                primary: true,
            },
            CardAction {
                id: "decline".into(),
                label_key: "interaction_decline".into(),
                primary: false,
            },
        ]
    } else if retry {
        vec![CardAction {
            id: "retry".into(),
            label_key: "interaction_retry".into(),
            primary: true,
        }]
    } else {
        vec![]
    };
    Card {
        message_id: message_id.into(),
        title,
        localized_title,
        status_key: status.into(),
        details,
        actions,
        editable: ready,
        error: if resolution.is_some() {
            None
        } else {
            submission
                .and_then(|s| s.error.clone())
                .or_else(|| errors.get("").cloned())
        },
        fields: fields
            .into_iter()
            .map(|field| CardField {
                value: values.get(&field.id).cloned().unwrap_or_default(),
                error_key: if ready {
                    errors.get(&field.id).cloned()
                } else {
                    None
                },
                localized_label: localized_title,
                field,
            })
            .collect(),
    }
}

impl Card {
    pub(crate) fn estimated_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.message_id.capacity()
            + self.title.capacity()
            + self.status_key.capacity()
            + self.error.as_ref().map_or(0, String::capacity)
            + self
                .details
                .iter()
                .map(|d| {
                    std::mem::size_of::<Detail>() + d.label_key.capacity() + d.value.capacity()
                })
                .sum::<usize>()
            + self
                .actions
                .iter()
                .map(|a| {
                    std::mem::size_of::<CardAction>() + a.id.capacity() + a.label_key.capacity()
                })
                .sum::<usize>()
            + self
                .fields
                .iter()
                .map(|f| {
                    std::mem::size_of::<CardField>()
                        + f.field.id.capacity()
                        + f.field.label.capacity()
                        + f.field.default.capacity()
                        + f.value.capacity()
                        + f.error_key.as_ref().map_or(0, String::capacity)
                        + f.field
                            .options
                            .iter()
                            .map(|o| {
                                std::mem::size_of::<Choice>()
                                    + o.label.capacity()
                                    + o.value.capacity()
                            })
                            .sum::<usize>()
                })
                .sum::<usize>()
    }
}

#[cfg(not(target_family = "wasm"))]
pub(crate) fn view(
    metadata: &crate::api::MessageMetadata,
    submission: Option<&Submission>,
    errors: &BTreeMap<String, String>,
) -> Option<Box<Card>> {
    let raw = metadata.interaction.as_deref()?;
    if let Some(request) = request(metadata) {
        Some(Box::new(card(
            metadata.id.as_deref()?,
            &request,
            metadata.interaction_result.as_deref(),
            submission,
            errors,
        )))
    } else if MessageContent::parse(raw).is_none() {
        Some(Box::new(Card {
            message_id: metadata.id.clone().unwrap_or_default(),
            title: "interaction_unsupported".into(),
            localized_title: true,
            status_key: "interaction_unsupported".into(),
            fields: vec![],
            details: vec![],
            actions: vec![],
            editable: false,
            error: None,
        }))
    } else {
        None
    }
}

#[cfg(not(target_family = "wasm"))]
pub(crate) fn request(metadata: &crate::api::MessageMetadata) -> Option<Request> {
    match MessageContent::parse(metadata.interaction.as_deref()?)?.content {
        Content::Request { request } if request.validate().is_ok() => Some(request),
        _ => None,
    }
}

#[cfg(not(target_family = "wasm"))]
pub(crate) fn result(metadata: &crate::api::MessageMetadata) -> Option<Resolution> {
    if metadata.author_kind != Some(zork_client_types::chat::AuthorKind::System) {
        return None;
    }
    match MessageContent::parse(metadata.interaction.as_deref()?)?.content {
        Content::Result { result }
            if valid_id(&result.request_message_id) && result.revision > 0 =>
        {
            Some(result)
        }
        _ => None,
    }
}

#[cfg(not(target_family = "wasm"))]
pub(crate) fn merge_result(
    metadata: &mut crate::api::MessageMetadata,
    incoming: &Resolution,
) -> anyhow::Result<bool> {
    anyhow::ensure!(
        metadata.id.as_deref() == Some(&incoming.request_message_id) && request(metadata).is_some(),
        "interaction_request_mismatch"
    );
    if let Some(old) = &metadata.interaction_result {
        if old.revision > incoming.revision {
            return Ok(false);
        }
        if old.revision == incoming.revision {
            anyhow::ensure!(old.as_ref() == incoming, "interaction_result_conflict");
            return Ok(false);
        }
    }
    metadata.interaction_result = Some(Box::new(incoming.clone()));
    Ok(true)
}
