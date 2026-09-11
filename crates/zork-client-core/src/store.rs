//! Device-local state exists independently of any running Gateway.
mod interaction_commands;
mod messages;
pub(crate) use interaction_commands::InteractionDelivery;
mod operations;
mod replica;
use anyhow::Result;
pub use replica::{ReplicaApply, ReplicaState};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{path::Path, sync::Mutex};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedNode {
    pub id: String,
    pub name: String,
    pub url: String,
    pub token: Option<String>,
    pub local: bool,
    #[serde(default)]
    pub mesh: Option<RemoteNode>,
    #[serde(default)]
    pub group: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RemoteNode {
    pub origin: String,
    pub addr: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueuedMessage {
    pub request_id: String,
    pub session_id: String,
    pub content: String,
    #[serde(default = "legacy_delivery_uncertain")]
    pub attempted: bool,
    #[serde(default)]
    pub sent_at_ms: u64,
    #[serde(default)]
    pub error: Option<String>,
}
impl QueuedMessage {
    pub fn delivery_status(&self) -> &'static str {
        if self.error.is_some() {
            "failed"
        } else if self.sent_at_ms > 0 && delivery_now_ms().saturating_sub(self.sent_at_ms) >= 1000 {
            "sending"
        } else {
            ""
        }
    }
}
pub fn delivery_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// Older clients did not record delivery attempts. Treat those receipts as
// uncertain, so upgrading never exposes a false "withdraw unsent" action.
fn legacy_delivery_uncertain() -> bool {
    true
}
pub struct ClientStore(
    Mutex<Connection>,
    pub(crate) Mutex<std::collections::HashMap<String, std::sync::Weak<crate::state::Device>>>,
    tokio::sync::watch::Sender<u64>,
    Mutex<std::collections::HashMap<String, std::sync::Arc<zork_observe::ValueSource<()>>>>,
);
impl ClientStore {
    pub(crate) fn submit_current_draft(
        &self,
        peer: &str,
        session: &str,
        text: String,
    ) -> Result<QueuedMessage> {
        let mut conn = self.0.lock().unwrap();
        let tx = conn.transaction()?;
        let key = format!("draft:{session}");
        let raw: Option<String> = tx
            .query_row(
                "SELECT value FROM cache WHERE node=?1 AND key=?2",
                params![peer, key],
                |r| r.get(0),
            )
            .optional()?;
        let raw = raw
            .map(|s| serde_json::from_str::<String>(&s))
            .transpose()?
            .unwrap_or_default();
        let mut draft = crate::state::Draft::decode(raw);
        draft.text = text;
        let content = zork_client_types::files::compose(
            &crate::comments::compose_document(&draft.text, &draft.comments, &draft.attachments),
            &draft.files,
        );
        crate::valid_content(&content, false)?;
        let message = QueuedMessage {
            request_id: ulid::Ulid::new().to_string(),
            session_id: session.into(),
            content,
            attempted: false,
            sent_at_ms: delivery_now_ms(),
            ..Default::default()
        };
        tx.execute(
            "INSERT INTO outbox(node,request_id,value) VALUES(?1,?2,?3)",
            params![peer, message.request_id, serde_json::to_string(&message)?],
        )?;
        for (key, value) in [(key, "\"\""), (format!("draft-comments:{session}"), "[]")] {
            tx.execute("INSERT INTO cache(node,key,value) VALUES(?1,?2,?3) ON CONFLICT(node,key) DO UPDATE SET value=excluded.value",params![peer,key,value])?;
        }
        tx.commit()?;
        drop(conn);
        self.delivery_changed();
        Ok(message)
    }
    pub(crate) fn edit_draft(
        &self,
        peer: &str,
        session: &str,
        action: crate::state::DraftAction,
    ) -> Result<()> {
        let mut conn = self.0.lock().unwrap();
        let tx = conn.transaction()?;
        let key = format!("draft:{session}");
        let raw: Option<String> = tx
            .query_row(
                "SELECT value FROM cache WHERE node=?1 AND key=?2",
                params![peer, key],
                |r| r.get(0),
            )
            .optional()?;
        let raw = raw
            .map(|s| serde_json::from_str::<String>(&s))
            .transpose()?
            .unwrap_or_default();
        let mut draft = crate::state::Draft::decode(raw);
        draft.apply(session, action)?;
        let value = serde_json::to_string(&draft.encoded())?;
        tx.execute("INSERT INTO cache(node,key,value) VALUES(?1,?2,?3) ON CONFLICT(node,key) DO UPDATE SET value=excluded.value",params![peer,key,value])?;
        tx.commit()?;
        Ok(())
    }
    pub(crate) fn delivery_events(&self) -> tokio::sync::watch::Receiver<u64> {
        self.2.subscribe()
    }
    fn delivery_changed(&self) {
        self.2
            .send_modify(|revision| *revision = revision.wrapping_add(1));
    }

    pub fn open(root: &Path) -> Result<Self> {
        std::fs::create_dir_all(root)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700))?;
        }
        let path = root.join("client.db");
        let conn = Connection::open(&path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        }
        conn.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE IF NOT EXISTS nodes(id TEXT PRIMARY KEY, value TEXT NOT NULL); CREATE TABLE IF NOT EXISTS cache(node TEXT NOT NULL,key TEXT NOT NULL,value TEXT NOT NULL,PRIMARY KEY(node,key)); CREATE TABLE IF NOT EXISTS outbox(node TEXT NOT NULL,request_id TEXT NOT NULL,value TEXT NOT NULL,PRIMARY KEY(node,request_id)); CREATE TABLE IF NOT EXISTS blobs(node TEXT NOT NULL,key TEXT NOT NULL,value BLOB NOT NULL,PRIMARY KEY(node,key));")?;
        conn.execute("UPDATE outbox SET value=json_set(value, '$.attempted', json('true'), '$.error', '发送中断，请手动重发') WHERE json_extract(value, '$.error') IS NULL", [])?;
        replica::initialize(&conn)?;
        operations::initialize(&conn)?;
        messages::initialize(&conn)?;
        interaction_commands::initialize(&conn)?;
        Ok(Self(
            Mutex::new(conn),
            Mutex::new(Default::default()),
            tokio::sync::watch::channel(0).0,
            Mutex::new(Default::default()),
        ))
    }
    /// Persist user intent separately from the lifetime of the node process.
    pub fn local_node_enabled(&self) -> Result<bool> {
        if let Some(enabled) = self.get("device", "local-node-enabled")? {
            return Ok(enabled);
        }
        // Older installs only recorded nodes that had actually been enabled.
        let enabled = self.nodes()?.iter().any(|node| node.local);
        self.set_local_node_enabled(enabled)?;
        Ok(enabled)
    }
    pub fn set_local_node_enabled(&self, enabled: bool) -> Result<()> {
        self.put("device", "local-node-enabled", &enabled)
    }
    pub fn put_blob(&self, node: &str, key: &str, bytes: &[u8]) -> Result<()> {
        self.0.lock().expect("client database").execute("INSERT INTO blobs(node,key,value) VALUES (?1,?2,?3) ON CONFLICT(node,key) DO UPDATE SET value=excluded.value",params![node,key,bytes])?;
        Ok(())
    }
    pub fn blob(&self, node: &str, key: &str) -> Result<Option<Vec<u8>>> {
        Ok(self
            .0
            .lock()
            .expect("client database")
            .query_row(
                "SELECT value FROM blobs WHERE node=?1 AND key=?2",
                params![node, key],
                |r| r.get(0),
            )
            .optional()?)
    }
    /// Commit one pending send and clear its source draft in the same database
    /// transaction, so a crash cannot leave both an outbox item and a sendable copy.
    pub fn enqueue_and_clear_draft(&self, node: &str, message: &QueuedMessage) -> Result<()> {
        let mut conn = self.0.lock().expect("client database");
        let transaction = conn.transaction()?;
        transaction.execute(
            "INSERT INTO outbox(node,request_id,value) VALUES (?1,?2,?3)",
            params![node, message.request_id, serde_json::to_string(message)?],
        )?;
        for (key, value) in [
            (format!("draft:{}", message.session_id), "\"\""),
            (format!("draft-comments:{}", message.session_id), "[]"),
        ] {
            transaction.execute("INSERT INTO cache(node,key,value) VALUES (?1,?2,?3) ON CONFLICT(node,key) DO UPDATE SET value=excluded.value",params![node,key,value])?;
        }
        transaction.commit()?;
        self.delivery_changed();
        Ok(())
    }
    pub fn enqueue(&self, node: &str, message: &QueuedMessage) -> Result<()> {
        self.0.lock().expect("client database").execute(
            "INSERT INTO outbox(node,request_id,value) VALUES (?1,?2,?3)",
            params![node, message.request_id, serde_json::to_string(message)?],
        )?;
        self.delivery_changed();
        Ok(())
    }
    pub fn outbox(&self, node: &str) -> Result<Vec<QueuedMessage>> {
        let conn = self.0.lock().expect("client database");
        let rows = conn
            .prepare("SELECT value FROM outbox WHERE node=?1 ORDER BY rowid")?
            .query_map([node], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|r| Ok(serde_json::from_str(&r)?))
            .collect()
    }
    pub fn acknowledge(&self, node: &str, id: &str) -> Result<()> {
        self.0.lock().expect("client database").execute(
            "DELETE FROM outbox WHERE node=?1 AND request_id=?2",
            params![node, id],
        )?;
        self.delivery_changed();
        Ok(())
    }
    /// A delivered Gateway message is also an authoritative acknowledgement,
    /// including when the POST response was lost.
    pub fn acknowledge_transcript(
        &self,
        node: &str,
        items: &[crate::api::TranscriptMessage],
    ) -> Result<()> {
        let ids: std::collections::HashSet<_> = items
            .iter()
            .filter_map(|m| {
                let crate::api::TranscriptMessage::Message { metadata, .. } = m;
                metadata.id.as_deref()
            })
            .collect();
        for pending in self.outbox(node)? {
            if ids
                .contains(format!("client-{}-{}", pending.session_id, pending.request_id).as_str())
            {
                self.acknowledge(node, &pending.request_id)?;
            }
        }
        Ok(())
    }
    pub fn begin_delivery(&self, node: &str, id: &str) -> Result<Option<QueuedMessage>> {
        let conn = self.0.lock().expect("client database");
        let value: Option<String> = conn
            .query_row(
                "SELECT value FROM outbox WHERE node=?1 AND request_id=?2",
                params![node, id],
                |r| r.get(0),
            )
            .optional()?;
        let Some(value) = value else {
            return Ok(None);
        };
        let mut message: QueuedMessage = serde_json::from_str(&value)?;
        if message.attempted || message.error.is_some() {
            return Ok(None);
        }
        message.attempted = true;
        conn.execute(
            "UPDATE outbox SET value=?3 WHERE node=?1 AND request_id=?2",
            params![node, id, serde_json::to_string(&message)?],
        )?;
        self.delivery_changed();
        Ok(Some(message))
    }
    pub fn fail_delivery(&self, node: &str, id: &str, error: &str) -> Result<()> {
        self.edit_delivery(node, id, |message| {
            message.error = Some(error.to_owned());
            Ok(())
        })
    }
    pub fn retry_delivery(&self, node: &str, id: &str) -> Result<()> {
        self.edit_delivery(node, id, |message| {
            anyhow::ensure!(
                message.error.is_some(),
                "Only failed messages can be resent"
            );
            message.error = None;
            message.attempted = false;
            message.sent_at_ms = delivery_now_ms();
            Ok(())
        })
    }
    pub fn delete_failed(&self, node: &str, id: &str) -> Result<()> {
        let conn = self.0.lock().expect("client database");
        let value: Option<String> = conn
            .query_row(
                "SELECT value FROM outbox WHERE node=?1 AND request_id=?2",
                params![node, id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(value) = value {
            let message: QueuedMessage = serde_json::from_str(&value)?;
            anyhow::ensure!(
                message.error.is_some(),
                "Only failed messages can be deleted"
            );
            conn.execute(
                "DELETE FROM outbox WHERE node=?1 AND request_id=?2",
                params![node, id],
            )?;
        }
        self.delivery_changed();
        Ok(())
    }
    fn edit_delivery(
        &self,
        node: &str,
        id: &str,
        edit: impl FnOnce(&mut QueuedMessage) -> Result<()>,
    ) -> Result<()> {
        let conn = self.0.lock().expect("client database");
        let value: Option<String> = conn
            .query_row(
                "SELECT value FROM outbox WHERE node=?1 AND request_id=?2",
                params![node, id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(value) = value {
            let mut message: QueuedMessage = serde_json::from_str(&value)?;
            edit(&mut message)?;
            conn.execute(
                "UPDATE outbox SET value=?3 WHERE node=?1 AND request_id=?2",
                params![node, id, serde_json::to_string(&message)?],
            )?;
        }
        self.delivery_changed();
        Ok(())
    }
    /// Delivery and cancellation serialize on the same database lock. Once a
    /// request may have reached Gateway, only its stable receipt can settle it.
    pub fn cancel_pending(&self, node: &str, id: &str) -> Result<Option<QueuedMessage>> {
        let conn = self.0.lock().expect("client database");
        let value: Option<String> = conn
            .query_row(
                "SELECT value FROM outbox WHERE node=?1 AND request_id=?2",
                params![node, id],
                |r| r.get(0),
            )
            .optional()?;
        let Some(value) = value else {
            return Ok(None);
        };
        let message: QueuedMessage = serde_json::from_str(&value)?;
        anyhow::ensure!(
            !message.attempted,
            "消息已尝试发送，需等待送达确认，不能将它视为已撤回"
        );
        conn.execute(
            "DELETE FROM outbox WHERE node=?1 AND request_id=?2",
            params![node, id],
        )?;
        self.delivery_changed();
        Ok(Some(message))
    }
    pub fn nodes(&self) -> Result<Vec<SavedNode>> {
        let conn = self.0.lock().expect("client database");
        let rows = conn
            .prepare("SELECT value FROM nodes ORDER BY rowid")?
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter().map(|row| {
            let mut node:SavedNode=serde_json::from_str(&row)?;
            let name:Option<String>=conn.query_row("SELECT json_extract(value,'$.name') FROM replica_entities WHERE peer=?1 AND scope=?2 AND kind='device' AND id='self' AND value IS NOT NULL",params![node.id,zork_client_types::sync::Scope::Catalog{}.key()],|r|r.get(0)).optional()?.flatten();
            if let Some(name)=name.filter(|name|!name.trim().is_empty()){node.name=name;}
            Ok(node)
        }).collect()
    }

    /// Withdraw a definitely-unsent message without losing it (or a newer draft)
    /// if the process dies. The transaction also serializes against delivery.
    pub fn withdraw_to_draft(&self, node: &str, id: &str) -> Result<Option<QueuedMessage>> {
        let mut conn = self.0.lock().expect("client database");
        let tx = conn.transaction()?;
        let value: Option<String> = tx
            .query_row(
                "SELECT value FROM outbox WHERE node=?1 AND request_id=?2",
                params![node, id],
                |r| r.get(0),
            )
            .optional()?;
        let Some(value) = value else {
            return Ok(None);
        };
        let message: QueuedMessage = serde_json::from_str(&value)?;
        anyhow::ensure!(
            !message.attempted,
            "消息已尝试发送，需等待送达确认，不能将它视为已撤回"
        );
        let key = format!("draft:{}", message.session_id);
        let previous: Option<String> = tx
            .query_row(
                "SELECT value FROM cache WHERE node=?1 AND key=?2",
                params![node, key],
                |r| r.get(0),
            )
            .optional()?;
        let previous: String = previous
            .map(|v| serde_json::from_str(&v))
            .transpose()?
            .unwrap_or_default();
        let restored = crate::comments::merge_drafts(&previous, &message.content);
        tx.execute("INSERT INTO cache(node,key,value) VALUES (?1,?2,?3) ON CONFLICT(node,key) DO UPDATE SET value=excluded.value",
            params![node, key, serde_json::to_string(&restored)?])?;
        tx.execute(
            "DELETE FROM outbox WHERE node=?1 AND request_id=?2",
            params![node, id],
        )?;
        tx.commit()?;
        self.delivery_changed();
        Ok(Some(message))
    }
    /// Commit accepted peers, network and removal of the invitation together.
    pub fn accept_invitation(&self, nodes: &[SavedNode], network: &crate::Network) -> Result<()> {
        self.commit_invitation(None, nodes, network)
    }
    pub(crate) fn accept_invitation_if(
        &self,
        id: &str,
        nodes: &[SavedNode],
        network: &crate::Network,
    ) -> Result<()> {
        self.commit_invitation(Some(id), nodes, network)
    }
    fn commit_invitation(
        &self,
        expected: Option<&str>,
        nodes: &[SavedNode],
        network: &crate::Network,
    ) -> Result<()> {
        let mut conn = self.0.lock().expect("client database");
        let tx = conn.transaction()?;
        if let Some(id) = expected {
            let pending: Option<String> = tx
                .query_row(
                    "SELECT value FROM cache WHERE node='device' AND key='invitation'",
                    [],
                    |row| row.get(0),
                )
                .optional()?;
            let pending: Option<serde_json::Value> = pending
                .map(|value| serde_json::from_str(&value))
                .transpose()?;
            anyhow::ensure!(
                pending
                    .as_ref()
                    .is_some_and(|value| value["invitation"]["id"] == id),
                "invitation_cancelled"
            );
        }
        for node in nodes {
            tx.execute("INSERT INTO nodes(id,value) VALUES (?1,?2) ON CONFLICT(id) DO UPDATE SET value=excluded.value",params![node.id,serde_json::to_string(node)?])?;
        }
        tx.execute("INSERT INTO cache(node,key,value) VALUES ('device','network',?1) ON CONFLICT(node,key) DO UPDATE SET value=excluded.value",[serde_json::to_string(network)?])?;
        tx.execute(
            "DELETE FROM cache WHERE node='device' AND key='invitation'",
            [],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn forget_invitation(&self) -> Result<()> {
        self.0.lock().expect("client database").execute(
            "DELETE FROM cache WHERE node='device' AND key='invitation'",
            [],
        )?;
        Ok(())
    }
    pub fn save_node(&self, node: &SavedNode) -> Result<()> {
        self.0.lock().expect("client database").execute("INSERT INTO nodes(id,value) VALUES (?1,?2) ON CONFLICT(id) DO UPDATE SET value=excluded.value",params![node.id,serde_json::to_string(node)?])?;
        Ok(())
    }
    /// Removing a connection preserves its local history, files and drafts.
    pub fn remove_node(&self, id: &str) -> Result<()> {
        self.0
            .lock()
            .expect("client database")
            .execute("DELETE FROM nodes WHERE id=?1", [id])?;
        Ok(())
    }
    pub fn put<T: Serialize>(&self, node: &str, key: &str, value: &T) -> Result<()> {
        let changed = self.0.lock().expect("client database").execute("INSERT INTO cache(node,key,value) VALUES (?1,?2,?3) ON CONFLICT(node,key) DO UPDATE SET value=excluded.value WHERE cache.value != excluded.value",params![node,key,serde_json::to_string(value)?])?;
        if changed > 0 && matches!(key, "public-settings" | "node-operation") {
            self.settings_changed(node);
        }
        Ok(())
    }
    pub(crate) fn settings_events(&self, node: &str) -> zork_observe::ValueSubscription<()> {
        self.3
            .lock()
            .unwrap()
            .entry(node.into())
            .or_insert_with(|| std::sync::Arc::new(zork_observe::ValueSource::new(())))
            .subscribe()
    }
    fn settings_changed(&self, node: &str) {
        let source = self.3.lock().unwrap().get(node).cloned();
        if let Some(source) = source {
            source.publish_changed((), zork_observe::Topics::ALL);
        }
    }
    pub fn get<T: serde::de::DeserializeOwned>(&self, node: &str, key: &str) -> Result<Option<T>> {
        let data: Option<String> = self
            .0
            .lock()
            .expect("client database")
            .query_row(
                "SELECT value FROM cache WHERE node=?1 AND key=?2",
                params![node, key],
                |r| r.get(0),
            )
            .optional()?;
        data.map(|data| Ok(serde_json::from_str(&data)?))
            .transpose()
    }
}

#[cfg(test)]
mod draft_transaction_tests {
    use super::*;
    #[test]
    fn queued_batch_clears_only_its_own_draft_and_failure_preserves_draft() {
        let root = tempfile::tempdir().unwrap();
        let store = ClientStore::open(root.path()).unwrap();
        store
            .put("a", "draft:conversation", &"pending text")
            .unwrap();
        store
            .put("a", "draft-comments:conversation", &vec!["comment"])
            .unwrap();
        store
            .put("b", "draft:conversation", &"other device")
            .unwrap();
        let message = QueuedMessage {
            request_id: "one".into(),
            session_id: "conversation".into(),
            content: "batch".into(),
            attempted: false,
            sent_at_ms: crate::store::delivery_now_ms(),
            ..Default::default()
        };
        store.enqueue_and_clear_draft("a", &message).unwrap();
        assert_eq!(
            store
                .get::<String>("a", "draft:conversation")
                .unwrap()
                .as_deref(),
            Some("")
        );
        assert!(store
            .get::<Vec<String>>("a", "draft-comments:conversation")
            .unwrap()
            .unwrap()
            .is_empty());
        assert_eq!(
            store
                .get::<String>("b", "draft:conversation")
                .unwrap()
                .as_deref(),
            Some("other device")
        );
        store.put("a", "draft:conversation", &"next draft").unwrap();
        assert!(store.enqueue_and_clear_draft("a", &message).is_err());
        assert_eq!(
            store
                .get::<String>("a", "draft:conversation")
                .unwrap()
                .as_deref(),
            Some("next draft")
        );
        assert_eq!(store.outbox("a").unwrap().len(), 1);
    }
}
