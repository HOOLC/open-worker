use super::Record;
use anyhow::{ensure, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::{path::Path, time::Duration};

pub struct Store(Connection);
impl Store {
    pub fn open(root: &Path) -> Result<Self> {
        std::fs::create_dir_all(root.join("state"))?;
        Self::from_connection(Connection::open(root.join("state/shared-services.sqlite"))?)
    }
    pub fn from_connection(db: Connection) -> Result<Self> {
        db.busy_timeout(Duration::from_secs(5))?;
        db.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
            CREATE TABLE IF NOT EXISTS shared_services (
                id TEXT PRIMARY KEY, owner TEXT NOT NULL, name TEXT NOT NULL,
                port INTEGER NOT NULL CHECK(port BETWEEN 1 AND 65535), UNIQUE(owner,name));
            CREATE TABLE IF NOT EXISTS service_requests (
                owner TEXT NOT NULL, request_id TEXT NOT NULL, fingerprint TEXT NOT NULL,
                service_id TEXT NOT NULL, PRIMARY KEY(owner,request_id));",
        )?;
        let has: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('shared_services') WHERE name='definition')", [], |row| row.get(0))?;
        if !has {
            db.execute_batch("ALTER TABLE shared_services ADD COLUMN definition TEXT")?;
        }
        Ok(Self(db))
    }
    pub fn load(&self) -> Result<Vec<Record>> {
        let mut query = self
            .0
            .prepare("SELECT id,owner,name,port,definition FROM shared_services ORDER BY id")?;
        let rows = query.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, u16>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })?;
        let mut records = Vec::new();
        for row in rows {
            let (id, owner, name, port, definition) = row?;
            let record = match definition {
                Some(definition) => {
                    let record: Record = serde_json::from_str(&definition)?;
                    ensure!(
                        record.id == id
                            && record.owner == owner
                            && record.name == name
                            && record.port == port,
                        "invalid_service_record"
                    );
                    record
                }
                None => Record::external(id, owner, name, port, true),
            };
            records.push(record);
        }
        Ok(records)
    }
    pub fn receipt(&self, owner: &str, request: &str, fingerprint: &str) -> Result<Option<String>> {
        let result: Option<(String,String)> = self.0.query_row("SELECT fingerprint,service_id FROM service_requests WHERE owner=?1 AND request_id=?2", params![owner,request], |r| Ok((r.get(0)?,r.get(1)?))).optional()?;
        if let Some((saved, id)) = result {
            ensure!(saved == fingerprint, "service_request_id_conflict");
            Ok(Some(id))
        } else {
            Ok(None)
        }
    }
    pub fn save(&mut self, record: &Record, receipt: Option<(&str, &str)>) -> Result<()> {
        let tx = self.0.transaction()?;
        tx.execute(
            "INSERT INTO shared_services(id,owner,name,port,definition) VALUES(?1,?2,?3,?4,?5)
            ON CONFLICT(id) DO UPDATE SET port=excluded.port,definition=excluded.definition",
            params![
                record.id,
                record.owner,
                record.name,
                record.port,
                serde_json::to_string(record)?
            ],
        )?;
        if let Some((request, fingerprint)) = receipt {
            tx.execute("INSERT INTO service_requests(owner,request_id,fingerprint,service_id) VALUES(?1,?2,?3,?4)", params![record.owner,request,fingerprint,record.id])?;
        }
        tx.commit()?;
        Ok(())
    }
    #[cfg(test)]
    pub fn read_only(&self) {
        self.0.execute_batch("PRAGMA query_only=ON").unwrap();
    }
}
