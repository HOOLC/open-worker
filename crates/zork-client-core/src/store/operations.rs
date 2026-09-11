//! Durable intent bookkeeping. Uncertain writes are only queried automatically;
//! sending again requires an explicit caller action with the same request ID.
use super::*;
use anyhow::ensure;
use zork_client_types::sync::{Mutation, Receipt};

pub(super) fn initialize(conn: &Connection) -> Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS client_operations(peer TEXT NOT NULL,id TEXT NOT NULL,intent TEXT NOT NULL,attempted INTEGER NOT NULL DEFAULT 0,receipt TEXT,PRIMARY KEY(peer,id));")?;
    Ok(())
}
impl ClientStore {
    pub fn prepare_operation(&self, peer: &str, intent: &Mutation) -> Result<()> {
        intent.validate().map_err(anyhow::Error::msg)?;
        let value = serde_json::to_string(intent)?;
        let mut conn = self.0.lock().expect("client database");
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT OR IGNORE INTO client_operations(peer,id,intent) VALUES(?1,?2,?3)",
            params![peer, intent.request_id, value],
        )?;
        let existing: String = tx.query_row(
            "SELECT intent FROM client_operations WHERE peer=?1 AND id=?2",
            params![peer, intent.request_id],
            |r| r.get(0),
        )?;
        ensure!(existing == value, "sync_request_id_reused");
        tx.commit()?;
        Ok(())
    }
    pub fn begin_operation(&self, peer: &str, id: &str) -> Result<bool> {
        let changed=self.0.lock().expect("client database").execute("UPDATE client_operations SET attempted=1 WHERE peer=?1 AND id=?2 AND attempted=0 AND receipt IS NULL",params![peer,id])?;
        Ok(changed == 1)
    }
    pub fn pending_operations(&self, peer: &str) -> Result<Vec<Mutation>> {
        let conn = self.0.lock().expect("client database");
        let rows=conn.prepare("SELECT intent FROM client_operations WHERE peer=?1 AND attempted=1 AND receipt IS NULL ORDER BY rowid LIMIT 16")?.query_map([peer],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|r| Ok(serde_json::from_str(&r)?))
            .collect()
    }
    pub fn operation_receipt(&self, peer: &str, id: &str) -> Result<Option<Receipt>> {
        let conn = self.0.lock().expect("client database");
        let value: Option<String> = conn
            .query_row(
                "SELECT receipt FROM client_operations WHERE peer=?1 AND id=?2",
                params![peer, id],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        value.map(|r| Ok(serde_json::from_str(&r)?)).transpose()
    }
    pub fn finish_operation(
        &self,
        peer: &str,
        owner: &str,
        generation: u64,
        receipt: &Receipt,
    ) -> Result<()> {
        ensure!(owner == receipt.owner, "sync_owner_mismatch");
        let mut conn = self.0.lock().expect("client database");
        let tx = conn.transaction()?;
        ensure!(
            super::replica::binding_generation(&tx, peer)? == generation,
            "sync_authorization_changed"
        );
        let value: String = tx.query_row(
            "SELECT intent FROM client_operations WHERE peer=?1 AND id=?2",
            params![peer, receipt.request_id],
            |r| r.get(0),
        )?;
        let intent: Mutation = serde_json::from_str(&value)?;
        ensure!(
            intent
                .owner
                .as_deref()
                .is_none_or(|owner| owner == receipt.owner),
            "sync_receipt_owner_mismatch"
        );
        if let Some(entity) = &receipt.entity {
            ensure!(
                (entity.kind, entity.id.as_str()) == intent.action.entity(),
                "sync_receipt_entity_mismatch"
            );
        }
        // A receipt settles intent, not the whole replication cursor. The next
        // catalog pull includes every object changed in the owner's transaction.
        super::replica::apply_receipt(&tx, peer, receipt)?;
        tx.execute(
            "UPDATE client_operations SET receipt=?3 WHERE peer=?1 AND id=?2",
            params![peer, receipt.request_id, serde_json::to_string(receipt)?],
        )?;
        tx.commit()?;
        Ok(())
    }
}
