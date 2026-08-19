use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

pub const SPOOL_DATABASE_FILENAME: &str = "spool.sqlite";
pub const SPOOL_SCHEMA_VERSION: i64 = 1;
pub const SPOOL_SCHEMA_NAME: &str = "spool_queue";
const BUSY_TIMEOUT_MS: u32 = 5_000;

#[derive(Clone, Debug)]
pub struct SpoolRow {
    pub id: String,
    #[allow(dead_code)]
    pub direction: String,
    #[allow(dead_code)]
    pub channel: String,
    pub payload: String,
    #[allow(dead_code)]
    pub receive_count: i64,
}

pub struct GatewayDb {
    conn: Mutex<Connection>,
}

impl GatewayDb {
    pub fn open(state_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(state_dir).context("create state dir")?;
        let path = state_dir.join(SPOOL_DATABASE_FILENAME);
        let conn = Connection::open(&path).with_context(|| format!("open {}", path.display()))?;
        conn.busy_timeout(std::time::Duration::from_millis(BUSY_TIMEOUT_MS as u64))?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        let conn = self.conn.lock().expect("db mutex");
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS schema_migrations (
              version INTEGER PRIMARY KEY,
              name TEXT NOT NULL DEFAULT '',
              applied_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS spool (
              id TEXT PRIMARY KEY,
              direction TEXT NOT NULL,
              channel TEXT NOT NULL,
              payload TEXT NOT NULL,
              locked_by TEXT,
              locked_at TEXT,
              lease_until TEXT,
              receive_count INTEGER NOT NULL DEFAULT 0,
              created_at TEXT NOT NULL,
              acked_at TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_spool_claim
              ON spool (direction, acked_at, lease_until, created_at);
            CREATE TABLE IF NOT EXISTS process_lease (
              role TEXT PRIMARY KEY,
              owner_id TEXT NOT NULL,
              lease_until TEXT NOT NULL,
              updated_at TEXT NOT NULL
            );
            "#,
        )?;
        let applied: Option<i64> = conn
            .query_row(
                "SELECT version FROM schema_migrations WHERE version = ?1",
                [SPOOL_SCHEMA_VERSION],
                |row| row.get(0),
            )
            .optional()?;
        if applied.is_none() {
            conn.execute(
                "INSERT INTO schema_migrations (version, name, applied_at) VALUES (?1, ?2, ?3)",
                params![SPOOL_SCHEMA_VERSION, SPOOL_SCHEMA_NAME, now_rfc3339()],
            )?;
        }
        Ok(())
    }

    pub fn enqueue_inbound(&self, id: &str, channel: &str, payload: &Value) -> Result<bool> {
        let conn = self.conn.lock().expect("db mutex");
        let changed = conn.execute(
            r#"
            INSERT INTO spool (id, direction, channel, payload, created_at)
            VALUES (?1, 'inbound', ?2, ?3, ?4)
            ON CONFLICT(id) DO NOTHING
            "#,
            params![id, channel, payload.to_string(), now_rfc3339()],
        )?;
        Ok(changed > 0)
    }

    pub fn claim(&self, direction: &str, owner: &str, lease_ms: i64) -> Result<Vec<SpoolRow>> {
        let conn = self.conn.lock().expect("db mutex");
        let now = now_rfc3339();
        let lease_until = rfc3339_from_millis(unix_ms() + lease_ms);
        conn.execute("BEGIN IMMEDIATE", [])?;
        let result = (|| -> Result<Vec<SpoolRow>> {
            conn.execute(
                r#"
                UPDATE spool
                SET locked_by = ?1,
                    locked_at = ?2,
                    lease_until = ?3,
                    receive_count = receive_count + 1
                WHERE id IN (
                  SELECT id FROM spool
                  WHERE direction = ?4
                    AND acked_at IS NULL
                    AND (locked_by IS NULL OR lease_until <= ?2)
                )
                "#,
                params![owner, now, lease_until, direction],
            )?;
            let mut stmt = conn.prepare(
                r#"
                SELECT id, direction, channel, payload, receive_count
                FROM spool
                WHERE direction = ?1 AND locked_by = ?2 AND acked_at IS NULL
                "#,
            )?;
            let rows = stmt
                .query_map(params![direction, owner], |row| {
                    Ok(SpoolRow {
                        id: row.get(0)?,
                        direction: row.get(1)?,
                        channel: row.get(2)?,
                        payload: row.get(3)?,
                        receive_count: row.get(4)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })();
        match result {
            Ok(rows) => {
                conn.execute("COMMIT", [])?;
                Ok(rows)
            }
            Err(error) => {
                let _ = conn.execute("ROLLBACK", []);
                Err(error)
            }
        }
    }

    pub fn ack(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().expect("db mutex");
        conn.execute(
            "UPDATE spool SET acked_at = ?1, locked_by = NULL, lease_until = NULL WHERE id = ?2",
            params![now_rfc3339(), id],
        )?;
        Ok(())
    }

    pub fn release(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().expect("db mutex");
        conn.execute("UPDATE spool SET locked_by = NULL, locked_at = NULL, lease_until = NULL WHERE id = ?1 AND acked_at IS NULL", [id])?;
        Ok(())
    }

    pub fn release_role(&self, role: &str, owner: &str) -> Result<()> {
        let conn = self.conn.lock().expect("db mutex");
        conn.execute(
            "DELETE FROM process_lease WHERE role = ?1 AND owner_id = ?2",
            [role, owner],
        )?;
        Ok(())
    }

    pub fn try_acquire_role(&self, role: &str, owner: &str, ttl_ms: i64) -> Result<bool> {
        let conn = self.conn.lock().expect("db mutex");
        let now = now_rfc3339();
        let until = rfc3339_from_millis(unix_ms() + ttl_ms);
        conn.execute("BEGIN IMMEDIATE", [])?;
        let acquired = (|| -> Result<bool> {
            let current: Option<(String, String)> = conn
                .query_row(
                    "SELECT owner_id, lease_until FROM process_lease WHERE role = ?1",
                    [role],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            let take = match current {
                None => true,
                Some((existing, lease_until)) => existing == owner || lease_until <= now,
            };
            if !take {
                return Ok(false);
            }
            conn.execute(
                r#"
                INSERT INTO process_lease (role, owner_id, lease_until, updated_at)
                VALUES (?1, ?2, ?3, ?4)
                ON CONFLICT(role) DO UPDATE SET
                  owner_id = excluded.owner_id,
                  lease_until = excluded.lease_until,
                  updated_at = excluded.updated_at
                "#,
                params![role, owner, until, now],
            )?;
            Ok(true)
        })();
        match acquired {
            Ok(value) => {
                conn.execute("COMMIT", [])?;
                Ok(value)
            }
            Err(error) => {
                let _ = conn.execute("ROLLBACK", []);
                Err(error)
            }
        }
    }
}

fn unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis() as i64)
        .unwrap_or(0)
}

fn now_rfc3339() -> String {
    rfc3339_from_millis(unix_ms())
}

fn rfc3339_from_millis(ms: i64) -> String {
    let seconds = ms.div_euclid(1000);
    let nanos = (ms.rem_euclid(1000) * 1_000_000) as u32;
    let days = seconds.div_euclid(86_400);
    let day_secs = seconds.rem_euclid(86_400) as u64;
    let (year, month, day) = civil_from_days(days);
    let hour = day_secs / 3600;
    let minute = (day_secs % 3600) / 60;
    let second = day_secs % 60;
    format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{:03}Z",
        nanos / 1_000_000
    )
}

fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 }.div_euclid(146_097);
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = (yoe as i64 + era * 400) as i32;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if month <= 2 { year + 1 } else { year };
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn claim_takes_all_unlocked_rows() {
        let dir = tempfile::tempdir().unwrap();
        let db = GatewayDb::open(dir.path()).unwrap();
        db.enqueue_inbound("e1", "slack", &json!({"event_id":"e1"}))
            .unwrap();
        db.enqueue_inbound("e2", "slack", &json!({"event_id":"e2"}))
            .unwrap();
        db.enqueue_inbound("e1", "slack", &json!({"event_id":"e1"}))
            .unwrap();

        let first = db.claim("inbound", "worker-a", 30_000).unwrap();
        assert_eq!(first.len(), 2);
        let ids: Vec<_> = first.iter().map(|row| row.id.as_str()).collect();
        assert!(ids.contains(&"e1"));
        assert!(ids.contains(&"e2"));

        let second = db.claim("inbound", "worker-b", 30_000).unwrap();
        assert!(second.is_empty());

        db.ack("e1").unwrap();
        db.release("e2").unwrap();
        let third = db.claim("inbound", "worker-b", 30_000).unwrap();
        assert_eq!(third.len(), 1);
        assert_eq!(third[0].id, "e2");
    }

    #[test]
    fn expired_lease_can_be_reclaimed() {
        let dir = tempfile::tempdir().unwrap();
        let db = GatewayDb::open(dir.path()).unwrap();
        db.enqueue_inbound("e1", "slack", &json!({"event_id":"e1"}))
            .unwrap();
        assert_eq!(db.claim("inbound", "worker-a", -1).unwrap().len(), 1);
        let claimed = db.claim("inbound", "worker-b", 30_000).unwrap();
        assert_eq!(claimed.len(), 1);
        assert_eq!(claimed[0].id, "e1");
        assert_eq!(claimed[0].receive_count, 2);
    }

    #[test]
    fn gateway_role_is_exclusive() {
        let dir = tempfile::tempdir().unwrap();
        let db = GatewayDb::open(dir.path()).unwrap();
        assert!(db.try_acquire_role("gateway", "gw-a", 30_000).unwrap());
        assert!(db.try_acquire_role("gateway", "gw-a", 30_000).unwrap());
        assert!(!db.try_acquire_role("gateway", "gw-b", 30_000).unwrap());
        assert!(db.try_acquire_role("gateway", "gw-a", -1).unwrap());
        assert!(db.try_acquire_role("gateway", "gw-b", 30_000).unwrap());
    }
}
