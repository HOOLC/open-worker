//! Versioned product replication. Cursors are scoped to an authenticated owner;
//! UI notification revisions and message ordering are not replication cursors.
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PROTOCOL: u16 = 1;
pub const MAX_PAGE_RECORDS: usize = 512;
pub const MAX_PAGE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Scope {
    Catalog {},
    Conversation { id: String },
}
impl Scope {
    pub fn key(&self) -> String {
        serde_json::to_string(self).expect("scope is serializable")
    }
    pub fn validate(&self) -> Result<(), String> {
        if let Self::Conversation { id } = self {
            identifier(id)?;
        }
        Ok(())
    }
    pub fn accepts(&self, kind: Kind) -> bool {
        match self {
            Self::Catalog {} => !matches!(kind, Kind::Message | Kind::Participant),
            Self::Conversation { .. } => matches!(kind, Kind::Message | Kind::Participant),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Device,
    Resource,
    Agent,
    Profile,
    Provider,
    Session,
    Task,
    Artifact,
    ReadMarker,
    Message,
    Participant,
}
impl Kind {
    pub fn key(self) -> &'static str {
        match self {
            Self::Device => "device",
            Self::Resource => "resource",
            Self::Agent => "agent",
            Self::Profile => "profile",
            Self::Provider => "provider",
            Self::Session => "session",
            Self::Task => "task",
            Self::Artifact => "artifact",
            Self::ReadMarker => "read_marker",
            Self::Message => "message",
            Self::Participant => "participant",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cursor {
    pub owner: String,
    pub epoch: String,
    pub scope: Scope,
    pub sequence: u64,
}
impl Cursor {
    pub fn validate(&self) -> Result<(), String> {
        identifier(&self.owner)?;
        identifier(&self.epoch)?;
        self.scope.validate()?;
        if self.sequence > i64::MAX as u64 {
            return Err("sync_sequence_overflow".into());
        }
        Ok(())
    }
    pub fn same_stream(&self, other: &Self) -> bool {
        self.owner == other.owner && self.epoch == other.epoch && self.scope == other.scope
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub kind: Kind,
    pub id: String,
    pub revision: u64,
    /// None is a durable deletion, not an unloaded value.
    pub value: Option<Value>,
}
impl Record {
    pub fn validate(&self, cursor: &Cursor) -> Result<(), String> {
        identifier(&self.id)?;
        if !cursor.scope.accepts(self.kind) {
            return Err("sync_record_outside_scope".into());
        }
        if self.revision == 0 || self.revision > cursor.sequence {
            return Err("sync_invalid_record_revision".into());
        }
        if self.value.as_ref().is_some_and(|v| !v.is_object()) {
            return Err("sync_record_must_be_object".into());
        }
        Ok(())
    }
}

/// Every page belongs to one stable batch. A snapshot has no `from` cursor.
/// A receiver stages pages and publishes the entire batch only after `last`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Page {
    pub protocol: u16,
    pub batch_id: String,
    pub from: Option<Cursor>,
    pub through: Cursor,
    pub index: u32,
    pub last: bool,
    pub records: Vec<Record>,
}
impl Page {
    pub fn validate(&self) -> Result<(), String> {
        if self.protocol != PROTOCOL {
            return Err("sync_unsupported_protocol".into());
        }
        identifier(&self.batch_id)?;
        self.through.validate()?;
        if let Some(from) = &self.from {
            from.validate()?;
            if !from.same_stream(&self.through) || from.sequence > self.through.sequence {
                return Err("sync_invalid_cursor_range".into());
            }
        }
        if self.records.len() > MAX_PAGE_RECORDS
            || serde_json::to_vec(self).map_err(|e| e.to_string())?.len() > MAX_PAGE_BYTES
        {
            return Err("sync_page_too_large".into());
        }
        for record in &self.records {
            record.validate(&self.through)?;
        }
        Ok(())
    }
    pub fn same_batch(&self, other: &Self) -> bool {
        self.protocol == other.protocol
            && self.batch_id == other.batch_id
            && self.from == other.from
            && self.through == other.through
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Continuation {
    pub batch_id: String,
    pub index: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pull {
    pub scope: Scope,
    pub after: Option<Cursor>,
    pub continuation: Option<Continuation>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum Reply {
    Page {
        page: Page,
    },
    ResetRequired {
        owner: String,
        epoch: String,
        reason: String,
    },
}

pub fn identifier(value: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > 1024 || value.chars().any(char::is_control) {
        Err("sync_invalid_identifier".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cursor() -> Cursor {
        Cursor {
            owner: "node".into(),
            epoch: "epoch".into(),
            scope: Scope::Catalog {},
            sequence: 3,
        }
    }
    #[test]
    fn cursors_bind_epoch_owner_and_scope() {
        let a = cursor();
        let mut b = a.clone();
        b.sequence = 2;
        assert!(a.same_stream(&b));
        b.epoch = "restored".into();
        assert!(!a.same_stream(&b));
        assert!(Record {
            kind: Kind::Message,
            id: "m".into(),
            revision: 1,
            value: None
        }
        .validate(&a)
        .is_err());
    }
    #[test]
    fn wire_rejects_unknown_scope_fields_and_invalid_revisions() {
        assert!(serde_json::from_str::<Scope>(r#"{"type":"catalog","secret":true}"#).is_err());
        assert!(Record {
            kind: Kind::Agent,
            id: "a".into(),
            revision: 4,
            value: None
        }
        .validate(&cursor())
        .is_err());
        assert!(Record {
            kind: Kind::Agent,
            id: "a".into(),
            revision: 3,
            value: Some(serde_json::json!({"name":"A"}))
        }
        .validate(&cursor())
        .is_ok());
    }
}

/// Idempotent product intents. Execution/runtime commands keep their existing
/// explicit delivery policy; a receipt lookup never resubmits an intent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mutation {
    pub request_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    pub epoch: String,
    pub expected_revision: u64,
    pub action: Action,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    AgentAvatar {
        id: String,
        avatar: String,
    },
    TaskDecision {
        id: String,
        expected_task_revision: i64,
        decision: Decision,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Accept,
    Reopen,
    Cancel,
}
impl Action {
    pub fn entity(&self) -> (Kind, &str) {
        match self {
            Self::AgentAvatar { id, .. } => (Kind::Agent, id),
            Self::TaskDecision { id, .. } => (Kind::Task, id),
        }
    }
}
impl Mutation {
    pub fn validate(&self) -> Result<(), String> {
        identifier(&self.request_id)?;
        if let Some(owner) = &self.owner {
            identifier(owner)?;
        }
        identifier(&self.epoch)?;
        identifier(self.action.entity().1)?;
        if self.expected_revision == 0 || self.expected_revision > i64::MAX as u64 {
            return Err("sync_invalid_expected_revision".into());
        }
        match &self.action {
            Action::AgentAvatar { avatar, .. } => identifier(avatar)?,
            Action::TaskDecision {
                expected_task_revision,
                ..
            } if *expected_task_revision < 0 => return Err("sync_invalid_task_revision".into()),
            _ => {}
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Applied,
    Conflict,
    Rejected,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub request_id: String,
    pub owner: String,
    pub epoch: String,
    pub outcome: Outcome,
    pub reason: Option<String>,
    pub entity: Option<Record>,
}
