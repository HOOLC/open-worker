//! Mutable projections of the append-only delivered message source. Original
//! source positions remain intact even when results fold into earlier cards.
use super::ClientStore;
use crate::api::{MessagePage, TranscriptMessage};
use anyhow::{ensure, Context, Result};
use rusqlite::{params, Connection, OptionalExtension, Transaction};

const LOCAL_CURSOR: &str = "zork-cache:";
#[cfg(test)]
mod interaction_tests;

pub(super) fn initialize(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS message_history(
            node TEXT NOT NULL, session TEXT NOT NULL, older_cursor TEXT,
            PRIMARY KEY(node,session));
         CREATE TABLE IF NOT EXISTS delivered_messages(
            node TEXT NOT NULL, session TEXT NOT NULL, id TEXT NOT NULL,
            position INTEGER NOT NULL, value TEXT NOT NULL,
            PRIMARY KEY(node,session,id), UNIQUE(node,session,position));
         CREATE TABLE IF NOT EXISTS pending_interaction_results(
            node TEXT NOT NULL, session TEXT NOT NULL, request_id TEXT NOT NULL,
            value TEXT NOT NULL, PRIMARY KEY(node,session,request_id));",
    )?;
    Ok(())
}

fn identity(message: &TranscriptMessage) -> Result<&str> {
    let TranscriptMessage::Message { metadata, .. } = message;
    metadata
        .id
        .as_deref()
        .filter(|id| !id.is_empty())
        .context("delivered message has no identity")
}

pub(super) fn authorized(conn: &Connection, node: &str, generation: u64) -> Result<()> {
    ensure!(
        super::replica::binding_generation(conn, node)? == generation,
        "message cache binding changed"
    );
    let revoked: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM replica_bindings WHERE peer=?1 AND revoked=1)",
        [node],
        |r| r.get(0),
    )?;
    ensure!(!revoked, "message cache access revoked");
    Ok(())
}

fn bounds(conn: &Connection, node: &str, session: &str) -> Result<(Option<i64>, Option<i64>)> {
    Ok(conn.query_row(
        "SELECT
            (SELECT position FROM delivered_messages WHERE node=?1 AND session=?2 ORDER BY position LIMIT 1),
            (SELECT position FROM delivered_messages WHERE node=?1 AND session=?2 ORDER BY position DESC LIMIT 1)",
        params![node,session], |r| Ok((r.get(0)?,r.get(1)?)),
    )?)
}

/// Pages must describe a contiguous range: catch-up overlaps the cached tail;
/// older pages extend its head. Never invent an ordering inside an unknown gap.
fn insert_page(
    tx: &Transaction<'_>,
    node: &str,
    session: &str,
    page: &MessagePage,
    older: bool,
) -> Result<bool> {
    let (min, max) = bounds(tx, node, session)?;
    let mut lookup = tx.prepare_cached(
        "SELECT position FROM delivered_messages WHERE node=?1 AND session=?2 AND id=?3",
    )?;
    let mut seen = std::collections::HashSet::new();
    let mut rows = Vec::new();
    for message in &page.items {
        let id = identity(message)?;
        if !seen.insert(id) {
            continue;
        }
        let position: Option<i64> = lookup
            .query_row(params![node, session, id], |r| r.get(0))
            .optional()?;
        rows.push((id, message, position));
    }
    let first = rows.iter().position(|(_, _, pos)| pos.is_some());
    let last = rows.iter().rposition(|(_, _, pos)| pos.is_some());
    if let (Some(first), Some(last)) = (first, last) {
        ensure!(
            rows[first..=last].iter().all(|(_, _, pos)| pos.is_some()),
            "message cache has an interior history gap"
        );
        ensure!(
            first == 0 || rows[first].2 == min,
            "history page does not overlap the cached head"
        );
        ensure!(
            last + 1 == rows.len() || rows[last].2 == max,
            "history page does not overlap the cached tail"
        );
    }
    let prefix = first.unwrap_or(if older { rows.len() } else { 0 });
    let mut head = min
        .unwrap_or(0)
        .checked_sub(prefix as i64)
        .context("message position overflow")?;
    let mut tail = max.unwrap_or(-1);
    let mut insert = tx.prepare_cached(
        "INSERT INTO delivered_messages(node,session,id,position,value) VALUES(?1,?2,?3,?4,?5)",
    )?;
    let mut first_position = None;
    for (index, (id, message, position)) in rows.iter().enumerate() {
        let position = match position {
            Some(position) => *position,
            None => {
                let position = if index < prefix {
                    let position = head;
                    head += 1;
                    position
                } else {
                    tail = tail.checked_add(1).context("message position overflow")?;
                    tail
                };
                insert.execute(params![
                    node,
                    session,
                    id,
                    position,
                    serde_json::to_string(message)?
                ])?;
                position
            }
        };
        first_position.get_or_insert(position);
    }
    let interaction_changed = merge_interactions(tx, node, session, &page.items)?;
    let new_min = bounds(tx, node, session)?.0;
    if min.is_none()
        || (first_position.is_some() && first_position == new_min)
        || (older && rows.is_empty())
    {
        tx.execute("INSERT INTO message_history(node,session,older_cursor) VALUES(?1,?2,?3) ON CONFLICT(node,session) DO UPDATE SET older_cursor=excluded.older_cursor WHERE older_cursor IS NOT excluded.older_cursor",
            params![node,session,page.older_cursor])?;
    }
    Ok(interaction_changed)
}

fn migrate(tx: &Transaction<'_>, node: &str, session: &str) -> Result<()> {
    let exists: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM message_history WHERE node=?1 AND session=?2)",
        params![node, session],
        |r| r.get(0),
    )?;
    if exists {
        return Ok(());
    }
    let key = format!("messages:{session}");
    let http_key = format!("http:/v1/im/sessions/{session}/messages");
    let raw: Option<String> = tx
        .query_row(
            "SELECT value FROM cache WHERE node=?1 AND key=?2",
            params![node, key],
            |r| r.get(0),
        )
        .optional()?;
    let page = match raw {
        Some(raw) => Some(serde_json::from_str::<MessagePage>(&raw)?),
        None => {
            let raw: Option<String> = tx
                .query_row(
                    "SELECT value FROM cache WHERE node=?1 AND key=?2",
                    params![node, http_key],
                    |r| r.get(0),
                )
                .optional()?;
            raw.map(|raw| {
                let value: serde_json::Value = serde_json::from_str(&raw)?;
                Ok::<_, anyhow::Error>(serde_json::from_value::<MessagePage>(
                    value["body"].clone(),
                )?)
            })
            .transpose()?
        }
    };
    if let Some(mut page) = page {
        // Do not discard an old payload that cannot be migrated safely.
        for message in &page.items {
            identity(message)?;
        }
        let mut statement = tx.prepare(
            "SELECT request_id FROM outbox WHERE node=?1 AND json_extract(value,'$.session_id')=?2",
        )?;
        let pending = statement
            .query_map(params![node, session], |r| r.get::<_, String>(0))?
            .map(|id| id.map(|id| format!("client-{session}-{id}")))
            .collect::<rusqlite::Result<std::collections::HashSet<_>>>()?;
        page.items
            .retain(|message| identity(message).is_ok_and(|id| !pending.contains(id)));
        insert_page(tx, node, session, &page, false)?;
        tx.execute(
            "DELETE FROM cache WHERE node=?1 AND key IN (?2,?3)",
            params![node, key, http_key],
        )?;
    }
    Ok(())
}

impl ClientStore {
    /// Store only newly delivered identities, atomically with the history boundary.
    pub fn cache_message_page(
        &self,
        node: &str,
        session: &str,
        page: &MessagePage,
        before: Option<&str>,
    ) -> Result<()> {
        self.cache_message_page_at(node, session, page, before, self.replica_generation(node)?)
    }

    pub(crate) fn cache_message_page_at(
        &self,
        node: &str,
        session: &str,
        page: &MessagePage,
        before: Option<&str>,
        generation: u64,
    ) -> Result<()> {
        let mut conn = self.0.lock().expect("client database");
        let tx = conn.transaction()?;
        authorized(&tx, node, generation)?;
        migrate(&tx, node, session)?;
        if let Some(before) = before {
            let cursor: Option<String> = tx
                .query_row(
                    "SELECT older_cursor FROM message_history WHERE node=?1 AND session=?2",
                    params![node, session],
                    |r| r.get(0),
                )
                .optional()?
                .flatten();
            // Another reader may already have extended this range.
            if cursor.as_deref() != Some(before) {
                return Ok(());
            }
        }
        let changed = insert_page(&tx, node, session, page, before.is_some())?;
        tx.commit()?;
        drop(conn);
        if changed {
            self.delivery_changed();
        }
        Ok(())
    }

    pub(crate) fn cache_delivered_message(
        &self,
        node: &str,
        session: &str,
        message: &TranscriptMessage,
        generation: u64,
    ) -> Result<()> {
        let mut conn = self.0.lock().expect("client database");
        let tx = conn.transaction()?;
        authorized(&tx, node, generation)?;
        migrate(&tx, node, session)?;
        let id = identity(message)?;
        // A replay cannot replace an already merged card with its initial request.
        tx.execute("INSERT INTO delivered_messages(node,session,id,position,value)
            SELECT ?1,?2,?3,COALESCE(MAX(position),-1)+1,?4 FROM delivered_messages WHERE node=?1 AND session=?2
            ON CONFLICT(node,session,id) DO NOTHING",params![node,session,id,serde_json::to_string(message)?])?;
        let changed = merge_interactions(&tx, node, session, std::slice::from_ref(message))?;
        tx.commit()?;
        drop(conn);
        if changed {
            self.delivery_changed();
        }
        Ok(())
    }

    /// Read only the card records touched by this incoming batch. Results keep
    /// their original source rows, so history cursors never follow display folds.
    pub(crate) fn cached_interaction_updates(
        &self,
        node: &str,
        session: &str,
        items: &[TranscriptMessage],
        generation: u64,
    ) -> Result<Vec<TranscriptMessage>> {
        let ids = interaction_roots(items);
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let conn = self.0.lock().expect("client database");
        authorized(&conn, node, generation)?;
        ids.iter()
            .filter_map(|id| match cached_message(&conn, node, session, id) {
                Ok(Some(message)) => Some(Ok(message)),
                Ok(None) => None,
                Err(error) => Some(Err(error)),
            })
            .collect()
    }

    pub(crate) fn cached_message_at(
        &self,
        node: &str,
        session: &str,
        id: &str,
        generation: u64,
    ) -> Result<Option<TranscriptMessage>> {
        let conn = self.0.lock().expect("client database");
        authorized(&conn, node, generation)?;
        cached_message(&conn, node, session, id)
    }

    /// A local cursor is private to this store and must never reach the Gateway.
    pub fn cached_messages(
        &self,
        node: &str,
        session: &str,
        before: Option<&str>,
        limit: usize,
    ) -> Result<Option<MessagePage>> {
        self.cached_messages_at(node, session, before, limit, self.replica_generation(node)?)
    }

    pub(crate) fn cached_messages_at(
        &self,
        node: &str,
        session: &str,
        before: Option<&str>,
        limit: usize,
        generation: u64,
    ) -> Result<Option<MessagePage>> {
        ensure!(
            limit > 0 && limit < i64::MAX as usize,
            "invalid cached message limit"
        );
        let position = match before {
            Some(cursor) => match cursor.strip_prefix(LOCAL_CURSOR) {
                Some(position) => Some(position.parse::<i64>()?),
                None => return Ok(None),
            },
            None => None,
        };
        let mut conn = self.0.lock().expect("client database");
        let tx = conn.transaction()?;
        authorized(&tx, node, generation)?;
        migrate(&tx, node, session)?;
        let cursor: Option<Option<String>> = tx
            .query_row(
                "SELECT older_cursor FROM message_history WHERE node=?1 AND session=?2",
                params![node, session],
                |r| r.get(0),
            )
            .optional()?;
        let Some(remote_cursor) = cursor else {
            return Ok(None);
        };
        let mut statement = tx.prepare_cached("SELECT position,value FROM delivered_messages WHERE node=?1 AND session=?2 AND position < ?3 ORDER BY position DESC LIMIT ?4")?;
        let mut rows = statement
            .query_map(
                params![
                    node,
                    session,
                    position.unwrap_or(i64::MAX),
                    (limit + 1) as i64
                ],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let older_cursor = if rows.len() > limit {
            rows.pop();
            Some(format!("{LOCAL_CURSOR}{}", rows.last().unwrap().0))
        } else {
            remote_cursor
        };
        let items = rows
            .into_iter()
            .rev()
            .map(|(_, value)| serde_json::from_str(&value))
            .collect::<serde_json::Result<_>>()?;
        drop(statement);
        tx.commit()?;
        Ok(Some(MessagePage {
            items,
            older_cursor,
        }))
    }

    pub(crate) fn cached_cursor_before(
        &self,
        node: &str,
        session: &str,
        id: &str,
        generation: u64,
    ) -> Result<Option<String>> {
        let conn = self.0.lock().expect("client database");
        authorized(&conn, node, generation)?;
        let position: i64 = conn.query_row(
            "SELECT position FROM delivered_messages WHERE node=?1 AND session=?2 AND id=?3",
            params![node, session, id],
            |r| r.get(0),
        )?;
        let older: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM delivered_messages WHERE node=?1 AND session=?2 AND position<?3)",params![node,session,position],|r|r.get(0))?;
        if older {
            return Ok(Some(format!("{LOCAL_CURSOR}{position}")));
        }
        Ok(conn.query_row(
            "SELECT older_cursor FROM message_history WHERE node=?1 AND session=?2",
            params![node, session],
            |r| r.get(0),
        )?)
    }
}

fn cached_message(
    conn: &Connection,
    node: &str,
    session: &str,
    id: &str,
) -> Result<Option<TranscriptMessage>> {
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM delivered_messages WHERE node=?1 AND session=?2 AND id=?3",
            params![node, session, id],
            |r| r.get(0),
        )
        .optional()?;
    value
        .map(|v| serde_json::from_str(&v).map_err(Into::into))
        .transpose()
}

fn interaction_roots(items: &[TranscriptMessage]) -> std::collections::BTreeSet<String> {
    items
        .iter()
        .filter_map(|message| {
            let TranscriptMessage::Message { metadata, .. } = message;
            if crate::interactions::request(metadata).is_some() {
                metadata.id.clone()
            } else {
                crate::interactions::result(metadata).map(|r| r.request_message_id)
            }
        })
        .collect()
}

fn merge_interactions(
    tx: &Transaction<'_>,
    node: &str,
    session: &str,
    items: &[TranscriptMessage],
) -> Result<bool> {
    use crate::interactions;
    let mut changed = false;
    for message in items {
        let TranscriptMessage::Message { metadata, .. } = message;
        if let Some(incoming) = interactions::result(metadata) {
            if let Some(mut target) =
                cached_message(tx, node, session, &incoming.request_message_id)?
            {
                let TranscriptMessage::Message { metadata, .. } = &mut target;
                if interactions::merge_result(metadata, &incoming)? {
                    tx.execute("UPDATE delivered_messages SET value=?4 WHERE node=?1 AND session=?2 AND id=?3", params![node, session, incoming.request_message_id, serde_json::to_string(&target)?])?;
                    changed = true;
                }
                tx.execute("DELETE FROM pending_interaction_results WHERE node=?1 AND session=?2 AND request_id=?3", params![node, session, incoming.request_message_id])?;
            } else {
                let old: Option<String> = tx.query_row("SELECT value FROM pending_interaction_results WHERE node=?1 AND session=?2 AND request_id=?3", params![node, session, incoming.request_message_id], |r| r.get(0)).optional()?;
                let old = old
                    .map(|s| serde_json::from_str::<interactions::Resolution>(&s))
                    .transpose()?;
                if let Some(old) = &old {
                    if old.revision == incoming.revision {
                        ensure!(old == &incoming, "interaction_result_conflict");
                    }
                }
                if old
                    .as_ref()
                    .is_none_or(|old| old.revision < incoming.revision)
                {
                    tx.execute("INSERT INTO pending_interaction_results VALUES(?1,?2,?3,?4) ON CONFLICT(node,session,request_id) DO UPDATE SET value=excluded.value", params![node, session, incoming.request_message_id, serde_json::to_string(&incoming)?])?;
                    changed = true;
                }
            }
            changed |= tx.execute(
                "DELETE FROM interaction_outbox WHERE node=?1 AND session=?2 AND message_id=?3",
                params![node, session, incoming.request_message_id],
            )? > 0;
        }
    }
    for message in items {
        let TranscriptMessage::Message { metadata, .. } = message;
        if interactions::request(metadata).is_none() {
            continue;
        }
        let id = identity(message)?;
        let pending: Option<String> = tx.query_row("SELECT value FROM pending_interaction_results WHERE node=?1 AND session=?2 AND request_id=?3", params![node, session, id], |r| r.get(0)).optional()?;
        if let Some(pending) = pending {
            let result = serde_json::from_str(&pending)?;
            let mut target =
                cached_message(tx, node, session, id)?.context("interaction_request_missing")?;
            let TranscriptMessage::Message { metadata, .. } = &mut target;
            if interactions::merge_result(metadata, &result)? {
                tx.execute(
                    "UPDATE delivered_messages SET value=?4 WHERE node=?1 AND session=?2 AND id=?3",
                    params![node, session, id, serde_json::to_string(&target)?],
                )?;
                changed = true;
            }
            tx.execute("DELETE FROM pending_interaction_results WHERE node=?1 AND session=?2 AND request_id=?3", params![node, session, id])?;
        }
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{MessageMetadata, Role};

    fn page(ids: &[&str], cursor: Option<&str>) -> MessagePage {
        MessagePage {
            items: ids
                .iter()
                .map(|id| TranscriptMessage::Message {
                    role: Role::Assistant,
                    content: format!("message {id}"),
                    metadata: MessageMetadata {
                        id: Some((*id).into()),
                        ..Default::default()
                    },
                })
                .collect(),
            older_cursor: cursor.map(str::to_owned),
        }
    }
    fn ids(page: &MessagePage) -> Vec<&str> {
        page.items
            .iter()
            .map(|message| identity(message).unwrap())
            .collect()
    }

    #[test]
    fn multiple_chats_survive_restart_and_page_locally_in_gateway_order() {
        let directory = tempfile::tempdir().unwrap();
        let store = ClientStore::open(directory.path()).unwrap();
        // IDs deliberately sort differently from gateway history order.
        store
            .cache_message_page("node", "a", &page(&["z", "a", "m"], Some("remote")), None)
            .unwrap();
        store
            .cache_message_page("node", "b", &page(&["m", "z"], None), None)
            .unwrap();
        store
            .cache_message_page("other-node", "a", &page(&["other"], None), None)
            .unwrap();
        store
            .cache_message_page("node", "a", &page(&["old2", "old1"], None), Some("remote"))
            .unwrap();
        store
            .cache_message_page(
                "node",
                "a",
                &page(&["a", "m", "new"], Some("irrelevant")),
                None,
            )
            .unwrap();
        drop(store);
        let store = ClientStore::open(directory.path()).unwrap();
        let tail = store
            .cached_messages("node", "a", None, 2)
            .unwrap()
            .unwrap();
        assert_eq!(ids(&tail), ["m", "new"]);
        let middle = store
            .cached_messages("node", "a", tail.older_cursor.as_deref(), 2)
            .unwrap()
            .unwrap();
        assert_eq!(ids(&middle), ["z", "a"]);
        let oldest = store
            .cached_messages("node", "a", middle.older_cursor.as_deref(), 2)
            .unwrap()
            .unwrap();
        assert_eq!(ids(&oldest), ["old2", "old1"]);
        assert!(oldest.older_cursor.is_none());
        assert_eq!(
            ids(&store
                .cached_messages("node", "b", None, 100)
                .unwrap()
                .unwrap()),
            ["m", "z"]
        );
        assert_eq!(
            ids(&store
                .cached_messages("other-node", "a", None, 100)
                .unwrap()
                .unwrap()),
            ["other"]
        );
    }

    #[test]
    fn replay_does_not_rewrite_messages_or_regress_the_history_boundary() {
        let directory = tempfile::tempdir().unwrap();
        let store = ClientStore::open(directory.path()).unwrap();
        let initial = page(&["m2", "m3"], Some("before2"));
        store
            .cache_message_page("node", "chat", &initial, None)
            .unwrap();
        store
            .cache_message_page(
                "node",
                "chat",
                &page(&["m1", "m2"], Some("before1")),
                Some("before2"),
            )
            .unwrap();
        let changes = store
            .0
            .lock()
            .unwrap()
            .query_row::<u64, _, _>("SELECT total_changes()", [], |r| r.get(0))
            .unwrap();
        store
            .cache_message_page("node", "chat", &initial, None)
            .unwrap();
        store
            .cache_delivered_message("node", "chat", &initial.items[1], 0)
            .unwrap();
        assert_eq!(
            store
                .0
                .lock()
                .unwrap()
                .query_row::<u64, _, _>("SELECT total_changes()", [], |r| r.get(0))
                .unwrap(),
            changes
        );
        let cached = store
            .cached_messages("node", "chat", None, 100)
            .unwrap()
            .unwrap();
        assert_eq!(ids(&cached), ["m1", "m2", "m3"]);
        assert_eq!(cached.older_cursor.as_deref(), Some("before1"));
        assert!(store
            .cached_messages("node", "chat", Some("before1"), 100)
            .unwrap()
            .is_none());
        // Even a conflicting duplicate cannot overwrite the delivered payload.
        let mut duplicate = initial.items[0].clone();
        let TranscriptMessage::Message { content, .. } = &mut duplicate;
        *content = "changed".into();
        store
            .cache_delivered_message("node", "chat", &duplicate, 0)
            .unwrap();
        assert_eq!(
            store
                .cached_messages("node", "chat", None, 100)
                .unwrap()
                .unwrap(),
            cached
        );
    }

    #[test]
    fn migration_is_atomic_and_keeps_pending_rows_in_the_outbox() {
        let directory = tempfile::tempdir().unwrap();
        let store = ClientStore::open(directory.path()).unwrap();
        let pending = super::super::QueuedMessage {
            request_id: "request".into(),
            session_id: "chat".into(),
            content: "unsent".into(),
            attempted: false,
            sent_at_ms: 0,
            error: None,
        };
        store
            .0
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO outbox(node,request_id,value) VALUES('node','request',?1)",
                [serde_json::to_string(&pending).unwrap()],
            )
            .unwrap();
        store
            .put(
                "node",
                "messages:chat",
                &page(&["old", "client-chat-request"], Some("remote")),
            )
            .unwrap();
        store
            .put(
                "node",
                "http:/v1/im/sessions/http/messages",
                &serde_json::json!({"body":page(&["http-old"],None)}),
            )
            .unwrap();
        let migrated = store
            .cached_messages("node", "chat", None, 100)
            .unwrap()
            .unwrap();
        assert_eq!(ids(&migrated), ["old"]);
        assert_eq!(migrated.older_cursor.as_deref(), Some("remote"));
        assert_eq!(store.outbox("node").unwrap(), [pending]);
        assert!(store
            .get::<MessagePage>("node", "messages:chat")
            .unwrap()
            .is_none());
        assert_eq!(
            ids(&store
                .cached_messages("node", "http", None, 100)
                .unwrap()
                .unwrap()),
            ["http-old"]
        );

        let mut invalid = page(&["valid", "invalid"], None);
        let TranscriptMessage::Message { metadata, .. } = &mut invalid.items[1];
        metadata.id = None;
        store.put("node", "messages:invalid", &invalid).unwrap();
        assert!(store.cached_messages("node", "invalid", None, 100).is_err());
        assert_eq!(
            store
                .get::<MessagePage>("node", "messages:invalid")
                .unwrap(),
            Some(invalid)
        );
        assert_eq!(
            bounds(&store.0.lock().unwrap(), "node", "invalid").unwrap(),
            (None, None)
        );
    }

    #[test]
    fn revoked_history_is_deleted_and_stale_writers_cannot_restore_it() {
        let directory = tempfile::tempdir().unwrap();
        let store = ClientStore::open(directory.path()).unwrap();
        let initial = page(&["m1"], None);
        store
            .cache_message_page("node", "chat", &initial, None)
            .unwrap();
        store
            .cache_message_page("other", "chat", &initial, None)
            .unwrap();
        store.revoke_replica("node").unwrap();
        assert!(store.cached_messages("node", "chat", None, 100).is_err());
        assert!(store
            .cache_message_page("node", "chat", &initial, None)
            .is_err());
        assert_eq!(
            bounds(&store.0.lock().unwrap(), "node", "chat").unwrap(),
            (None, None)
        );
        assert!(store
            .cached_messages("other", "chat", None, 100)
            .unwrap()
            .is_some());
        store
            .0
            .lock()
            .unwrap()
            .execute(
                "UPDATE replica_bindings SET revoked=0 WHERE peer='node'",
                [],
            )
            .unwrap();
        assert!(store
            .cache_message_page_at("node", "chat", &initial, None, 0)
            .is_err());
        assert!(store
            .cache_delivered_message("node", "chat", &initial.items[0], 0)
            .is_err());
        assert!(store
            .cached_messages("node", "chat", None, 100)
            .unwrap()
            .is_none());
        store
            .cache_message_page("node", "chat", &initial, None)
            .unwrap();
    }

    #[test]
    fn rejects_interior_gaps_without_publishing_partial_rows() {
        let directory = tempfile::tempdir().unwrap();
        let store = ClientStore::open(directory.path()).unwrap();
        let initial = page(&["a", "c"], Some("remote"));
        store
            .cache_message_page("node", "chat", &initial, None)
            .unwrap();
        assert!(store
            .cache_message_page("node", "chat", &page(&["older", "a", "b", "c"], None), None)
            .is_err());
        assert_eq!(
            store
                .cached_messages("node", "chat", None, 100)
                .unwrap()
                .unwrap(),
            initial
        );
    }
}
