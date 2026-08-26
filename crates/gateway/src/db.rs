use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

use crate::config::now_rfc3339;

pub(crate) const GATEWAY_DB: &str = "gateway.sqlite";
const BUSY_TIMEOUT_MS: u32 = 5_000;

pub struct GatewayDb {
    conn: Mutex<Connection>,
    workspaces_root: PathBuf,
}

#[derive(Clone, Debug)]
pub struct SessionRow {
    pub key: String,
    pub id: Option<String>,
    pub channel_id: String,
    pub channel_name: Option<String>,
    pub channel_type: Option<String>,
    pub root_thread_ts: String,
    pub workspace_path: String,
    pub updated_at: String,
    pub created_at: String,
    pub profile_id: Option<String>,
    pub model: Option<String>,
    pub thinking: Option<String>,
    pub selection_blocked_at: Option<String>,
    pub selection_block_reason: Option<String>,
    pub last_slack_reply_at: Option<String>,
    pub initiator_user_id: Option<String>,
}

#[derive(Clone, Debug)]
pub struct InboundRow {
    pub session_key: String,
    pub message_ts: String,
    pub source: String,
    pub user_id: String,
    pub text: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct JobRow {
    pub id: String,
    pub token: String,
    pub session_key: String,
    pub channel_id: String,
    pub root_thread_ts: String,
    pub kind: String,
    pub shell: String,
    pub cwd: String,
    pub script_path: String,
    pub restart_on_boot: bool,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

impl GatewayDb {
    pub fn open(state_dir: &Path, workspaces_root: &Path) -> Result<Self> {
        fs::create_dir_all(state_dir).context("create state dir")?;
        fs::create_dir_all(workspaces_root).context("create workspaces root")?;
        let path = state_dir.join(GATEWAY_DB);
        let conn = Connection::open(&path).with_context(|| format!("open {}", path.display()))?;
        conn.busy_timeout(std::time::Duration::from_millis(BUSY_TIMEOUT_MS as u64))?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        let db = Self {
            conn: Mutex::new(conn),
            workspaces_root: workspaces_root.to_path_buf(),
        };
        db.initialize_schema()?;
        Ok(db)
    }

    fn initialize_schema(&self) -> Result<()> {
        {
            let conn = self.conn.lock().expect("db mutex");
            conn.execute_batch(
                r#"
            CREATE TABLE IF NOT EXISTS sessions (
              key TEXT PRIMARY KEY,
              id TEXT UNIQUE,
              channel_id TEXT NOT NULL,
              channel_name TEXT,
              channel_type TEXT,
              root_thread_ts TEXT NOT NULL,
              workspace_path TEXT NOT NULL,
              initiator_user_id TEXT,
              initiator_message_ts TEXT,
              initiator_captured_at TEXT,
              created_at TEXT NOT NULL,
              updated_at TEXT NOT NULL,
              last_observed_message_ts TEXT,
              last_delivered_message_ts TEXT,
              last_slack_reply_at TEXT,
              profile_id TEXT,
              model TEXT,
              thinking TEXT,
              selection_bound_at TEXT,
              selection_blocked_at TEXT,
              selection_block_reason TEXT,
              UNIQUE(channel_id, root_thread_ts)
            );
            CREATE TABLE IF NOT EXISTS inbound_messages (
              key TEXT NOT NULL UNIQUE,
              session_key TEXT NOT NULL REFERENCES sessions(key) ON DELETE CASCADE,
              channel_id TEXT NOT NULL,
              channel_type TEXT,
              root_thread_ts TEXT NOT NULL,
              message_ts TEXT NOT NULL,
              source TEXT NOT NULL,
              user_id TEXT NOT NULL,
              text TEXT NOT NULL,
              status TEXT NOT NULL,
              created_at TEXT NOT NULL,
              updated_at TEXT NOT NULL,
              PRIMARY KEY(session_key, message_ts)
            );
            CREATE TABLE IF NOT EXISTS background_jobs (
              id TEXT PRIMARY KEY,
              token TEXT NOT NULL,
              session_key TEXT NOT NULL REFERENCES sessions(key) ON DELETE CASCADE,
              channel_id TEXT NOT NULL,
              root_thread_ts TEXT NOT NULL,
              kind TEXT NOT NULL,
              shell TEXT NOT NULL,
              cwd TEXT NOT NULL,
              script_path TEXT NOT NULL,
              restart_on_boot INTEGER NOT NULL,
              status TEXT NOT NULL,
              created_at TEXT NOT NULL,
              updated_at TEXT NOT NULL,
              started_at TEXT,
              completed_at TEXT,
              cancelled_at TEXT,
              exit_code INTEGER,
              error TEXT,
              last_event_at TEXT,
              last_event_kind TEXT,
              last_event_summary TEXT
            );
            CREATE TABLE IF NOT EXISTS admin_events (
              sequence INTEGER PRIMARY KEY AUTOINCREMENT,
              kind TEXT NOT NULL,
              scope TEXT NOT NULL,
              session_key TEXT,
              entity_id TEXT,
              payload TEXT NOT NULL,
              created_at TEXT NOT NULL
            );
            "#,
            )?;
        }
        Ok(())
    }

    pub fn snapshot(&self) -> Result<Value> {
        let sessions = self.list_sessions()?;
        let listed: Vec<Value> = sessions
            .iter()
            .take(500)
            .map(|session| self.session_summary(session))
            .collect::<Result<_>>()?;
        let conn = self.conn.lock().expect("db mutex");
        let running_jobs: i64 = conn.query_row(
            "SELECT COUNT(*) FROM background_jobs WHERE status = 'running'",
            [],
            |row| row.get(0),
        )?;
        let cursor: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(sequence), 0) FROM admin_events",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        Ok(json!({
            "ok": true,
            "realtime": {
                "generatedAt": now_rfc3339(),
                "cursor": cursor,
            },
            "state": {
                "runningJobCount": running_jobs,
                "sessions": listed,
            }
        }))
    }

    pub fn session_summary(&self, session: &SessionRow) -> Result<Value> {
        let inbound = self.list_inbound(&session.key)?;
        let jobs = self.list_jobs_for_session(&session.key)?;
        let last_user = inbound.iter().rev().find(|row| !row.user_id.is_empty());
        Ok(json!({
            "key": session.key,
            "id": session.id,
            "platform": "slack",
            "conversationId": session.channel_id,
            "conversationKind": session.channel_type,
            "rootMessageId": session.root_thread_ts,
            "channelId": session.channel_id,
            "channelLabel": session.channel_name.as_deref().unwrap_or(&session.channel_id),
            "channelName": session.channel_name,
            "channelType": session.channel_type,
            "rootThreadTs": session.root_thread_ts,
            "threadUrl": format!(
                "https://slack.com/archives/{}/p{}",
                session.channel_id,
                session.root_thread_ts.replace('.', "")
            ),
            "workspacePath": session.workspace_path,
            "updatedAt": session.updated_at,
            "lastActivityAt": session.updated_at,
            "createdAt": session.created_at,
            "profileId": session.profile_id,
            "model": session.model,
            "thinking": session.thinking,
            "selectionBlockedAt": session.selection_blocked_at,
            "selectionBlockReason": session.selection_block_reason,
            "lastSlackReplyAt": session.last_slack_reply_at,
            "initiatorUserId": session.initiator_user_id,
            "lastUserMessage": last_user.map(|row| json!({
                "sessionKey": row.session_key,
                "messageTs": row.message_ts,
                "source": row.source,
                "status": row.status,
                "userId": row.user_id,
                "textPreview": row.text.chars().take(160).collect::<String>(),
                "updatedAt": row.updated_at,
            })),
            "blockedInboundCount": inbound.iter().filter(|row| row.status == "blocked").count(),
            "backgroundJobCount": jobs.len(),
            "runningBackgroundJobCount": jobs.iter().filter(|job| job.status == "running").count(),
            "failedBackgroundJobCount": jobs.iter().filter(|job| job.status == "failed").count(),
        }))
    }

    pub fn list_sessions(&self) -> Result<Vec<SessionRow>> {
        let conn = self.conn.lock().expect("db mutex");
        let mut stmt = conn.prepare(
            r#"
            SELECT key, id, channel_id, channel_name, channel_type, root_thread_ts, workspace_path,
                   updated_at, created_at, profile_id, model, thinking, last_slack_reply_at,
                   initiator_user_id, selection_blocked_at, selection_block_reason
            FROM sessions
            ORDER BY updated_at DESC
            "#,
        )?;
        let rows = stmt
            .query_map([], map_session_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn ensure_session(
        &self,
        channel_id: &str,
        root_thread_ts: &str,
        channel_type: Option<&str>,
        initiator_user_id: Option<&str>,
        initiator_message_ts: Option<&str>,
    ) -> Result<SessionRow> {
        let key = format!("{channel_id}:{root_thread_ts}");
        if let Some(_existing) = self.get_session(&key)? {
            if channel_type.is_some() || initiator_user_id.is_some() {
                self.set_channel_metadata(&key, None, channel_type)?;
            }
            return self
                .get_session(&key)?
                .context("session missing after ensure");
        }
        let now = now_rfc3339();
        let workspace_path = self
            .workspaces_root
            .join("slack")
            .join(channel_id)
            .join(root_thread_ts);
        fs::create_dir_all(&workspace_path).context("create Slack session workspace")?;
        let workspace_path = workspace_path.to_string_lossy().into_owned();
        let conn = self.conn.lock().expect("db mutex");
        conn.execute(
            r#"
            INSERT OR IGNORE INTO sessions (
              key, id, channel_id, channel_type, root_thread_ts, workspace_path, created_at, updated_at,
              initiator_user_id, initiator_message_ts, initiator_captured_at
            ) VALUES (?1, NULL, ?2, ?3, ?4, ?5, ?6, ?6, ?7, ?8, ?6)
            "#,
            params![
                key,
                channel_id,
                channel_type,
                root_thread_ts,
                workspace_path,
                now,
                initiator_user_id,
                initiator_message_ts,
            ],
        )?;
        drop(conn);
        self.get_session(&key)?
            .context("session missing after insert")
    }

    pub fn get_session(&self, key: &str) -> Result<Option<SessionRow>> {
        let conn = self.conn.lock().expect("db mutex");
        conn.query_row(
            r#"
            SELECT key, id, channel_id, channel_name, channel_type, root_thread_ts, workspace_path,
                   updated_at, created_at, profile_id, model, thinking, last_slack_reply_at,
                   initiator_user_id, selection_blocked_at, selection_block_reason
            FROM sessions WHERE key = ?1
            "#,
            [key],
            map_session_row,
        )
        .optional()
        .context("get session")
    }

    pub fn get_session_by_id(&self, id: &str) -> Result<Option<SessionRow>> {
        let conn = self.conn.lock().expect("db mutex");
        conn.query_row(
            r#"
            SELECT key, id, channel_id, channel_name, channel_type, root_thread_ts, workspace_path,
                   updated_at, created_at, profile_id, model, thinking, last_slack_reply_at,
                   initiator_user_id, selection_blocked_at, selection_block_reason
            FROM sessions WHERE id = ?1
            "#,
            [id],
            map_session_row,
        )
        .optional()
        .context("get session by id")
    }

    pub fn find_session_by_workspace(&self, cwd: &str) -> Result<Option<SessionRow>> {
        let sessions = self.list_sessions()?;
        let cwd = Path::new(cwd);
        Ok(sessions.into_iter().find(|session| {
            if session.workspace_path.is_empty() {
                return false;
            }
            let workspace = Path::new(&session.workspace_path);
            cwd == workspace || cwd.starts_with(workspace)
        }))
    }

    pub fn delete_session(&self, key: &str) -> Result<bool> {
        let conn = self.conn.lock().expect("db mutex");
        let changed = conn.execute("DELETE FROM sessions WHERE key = ?1", [key])?;
        Ok(changed > 0)
    }

    pub fn set_channel_metadata(
        &self,
        key: &str,
        channel_name: Option<&str>,
        channel_type: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.lock().expect("db mutex");
        conn.execute(
            r#"
            UPDATE sessions
            SET channel_name = COALESCE(?1, channel_name),
                channel_type = COALESCE(?2, channel_type),
                updated_at = ?3
            WHERE key = ?4
            "#,
            params![channel_name, channel_type, now_rfc3339(), key],
        )?;
        Ok(())
    }

    pub fn set_agent_session(
        &self,
        key: &str,
        session_id: &str,
        workspace_path: &str,
        profile_id: &str,
        model: &str,
        thinking: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().expect("db mutex");
        let now = now_rfc3339();
        conn.execute(
            r#"
            UPDATE sessions
            SET id = ?1,
                workspace_path = ?2,
                profile_id = ?3,
                model = ?4,
                thinking = ?5,
                selection_bound_at = ?6,
                selection_blocked_at = NULL,
                selection_block_reason = NULL,
                updated_at = ?6
            WHERE key = ?7
            "#,
            params![
                session_id,
                workspace_path,
                profile_id,
                model,
                thinking,
                now,
                key
            ],
        )?;
        Ok(())
    }

    pub fn clear_agent_session(&self, key: &str) -> Result<()> {
        let conn = self.conn.lock().expect("db mutex");
        let now = now_rfc3339();
        conn.execute(
            "UPDATE sessions SET id = NULL, profile_id = NULL, model = NULL, thinking = NULL, selection_bound_at = NULL, updated_at = ?1 WHERE key = ?2",
            params![now, key],
        )?;
        Ok(())
    }

    pub fn set_selection_block(&self, key: &str, reason: &str) -> Result<()> {
        let conn = self.conn.lock().expect("db mutex");
        let now = now_rfc3339();
        conn.execute(
            "UPDATE sessions SET selection_blocked_at = ?1, selection_block_reason = ?2, updated_at = ?1 WHERE key = ?3",
            params![now, reason, key],
        )?;
        Ok(())
    }

    pub fn inbound_status(&self, session_key: &str, message_ts: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().expect("db mutex");
        conn.query_row(
            "SELECT status FROM inbound_messages WHERE session_key = ?1 AND message_ts = ?2",
            params![session_key, message_ts],
            |row| row.get(0),
        )
        .optional()
        .context("get inbound status")
    }

    pub fn touch_reply(&self, key: &str) -> Result<()> {
        let conn = self.conn.lock().expect("db mutex");
        let now = now_rfc3339();
        conn.execute(
            "UPDATE sessions SET last_slack_reply_at = ?1, updated_at = ?1 WHERE key = ?2",
            params![now, key],
        )?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_inbound(
        &self,
        session_key: &str,
        channel_id: &str,
        root_thread_ts: &str,
        message_ts: &str,
        source: &str,
        user_id: &str,
        text: &str,
        channel_type: Option<&str>,
        status: &str,
    ) -> Result<()> {
        let now = now_rfc3339();
        let key = format!("{session_key}:{message_ts}");
        let conn = self.conn.lock().expect("db mutex");
        conn.execute(
            r#"
            INSERT INTO inbound_messages (
              key, session_key, channel_id, channel_type, root_thread_ts, message_ts, source, user_id, text, status, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)
            ON CONFLICT(session_key, message_ts) DO UPDATE SET
              text = excluded.text,
              status = excluded.status,
              updated_at = excluded.updated_at
            "#,
            params![
                key,
                session_key,
                channel_id,
                channel_type,
                root_thread_ts,
                message_ts,
                source,
                user_id,
                text,
                status,
                now
            ],
        )?;
        Ok(())
    }

    pub fn list_inbound(&self, session_key: &str) -> Result<Vec<InboundRow>> {
        let conn = self.conn.lock().expect("db mutex");
        let mut stmt = conn.prepare(
            "SELECT session_key, message_ts, source, user_id, text, status, created_at, updated_at FROM inbound_messages WHERE session_key = ?1 ORDER BY created_at ASC, message_ts ASC",
        )?;
        let rows = stmt
            .query_map([session_key], |row| {
                Ok(InboundRow {
                    session_key: row.get(0)?,
                    message_ts: row.get(1)?,
                    source: row.get(2)?,
                    user_id: row.get(3)?,
                    text: row.get(4)?,
                    status: row.get(5)?,
                    created_at: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn insert_job(&self, job: &JobRow) -> Result<()> {
        let conn = self.conn.lock().expect("db mutex");
        conn.execute(
            r#"
            INSERT INTO background_jobs (
              id, token, session_key, channel_id, root_thread_ts,
              kind, shell, cwd, script_path, restart_on_boot, status, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12)
            "#,
            params![
                job.id,
                job.token,
                job.session_key,
                job.channel_id,
                job.root_thread_ts,
                job.kind,
                job.shell,
                job.cwd,
                job.script_path,
                job.restart_on_boot as i64,
                job.status,
                job.created_at,
            ],
        )?;
        Ok(())
    }

    pub fn get_job(&self, id: &str) -> Result<Option<JobRow>> {
        let conn = self.conn.lock().expect("db mutex");
        conn.query_row(
            r#"
            SELECT id, token, session_key, channel_id, root_thread_ts, kind, shell, cwd, script_path,
                   restart_on_boot, status, created_at, updated_at
            FROM background_jobs WHERE id = ?1
            "#,
            [id],
            map_job_row,
        )
        .optional()
        .context("get job")
    }

    pub fn list_jobs(&self) -> Result<Vec<JobRow>> {
        let conn = self.conn.lock().expect("db mutex");
        let mut stmt = conn.prepare(
            r#"
            SELECT id, token, session_key, channel_id, root_thread_ts, kind, shell, cwd, script_path,
                   restart_on_boot, status, created_at, updated_at
            FROM background_jobs ORDER BY created_at DESC
            "#,
        )?;
        let rows = stmt
            .query_map([], map_job_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn list_jobs_for_session(&self, session_key: &str) -> Result<Vec<JobRow>> {
        let conn = self.conn.lock().expect("db mutex");
        let mut stmt = conn.prepare(
            r#"
            SELECT id, token, session_key, channel_id, root_thread_ts, kind, shell, cwd, script_path,
                   restart_on_boot, status, created_at, updated_at
            FROM background_jobs WHERE session_key = ?1 ORDER BY created_at DESC
            "#,
        )?;
        let rows = stmt
            .query_map([session_key], map_job_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn update_job_status(
        &self,
        id: &str,
        status: &str,
        error: Option<&str>,
        extra: Option<(&str, &str)>,
    ) -> Result<()> {
        let conn = self.conn.lock().expect("db mutex");
        let now = now_rfc3339();
        match status {
            "running" => {
                conn.execute(
                    "UPDATE background_jobs SET status = ?1, started_at = COALESCE(started_at, ?2), updated_at = ?2 WHERE id = ?3",
                    params![status, now, id],
                )?;
            }
            "cancelled" => {
                conn.execute(
                    "UPDATE background_jobs SET status = ?1, cancelled_at = ?2, completed_at = ?2, updated_at = ?2 WHERE id = ?3",
                    params![status, now, id],
                )?;
            }
            "failed" => {
                conn.execute(
                    "UPDATE background_jobs SET status = ?1, error = ?2, completed_at = ?3, updated_at = ?3 WHERE id = ?4",
                    params![status, error, now, id],
                )?;
            }
            "succeeded" => {
                conn.execute(
                    "UPDATE background_jobs SET status = ?1, completed_at = ?2, updated_at = ?2 WHERE id = ?3",
                    params![status, now, id],
                )?;
            }
            _ => {
                conn.execute(
                    "UPDATE background_jobs SET status = ?1, updated_at = ?2 WHERE id = ?3",
                    params![status, now, id],
                )?;
            }
        }
        if let Some((kind, summary)) = extra {
            conn.execute(
                "UPDATE background_jobs SET last_event_at = ?1, last_event_kind = ?2, last_event_summary = ?3 WHERE id = ?4",
                params![now, kind, summary, id],
            )?;
        }
        Ok(())
    }

    pub fn insert_admin_event(
        &self,
        kind: &str,
        scope: &str,
        session_key: Option<&str>,
        entity_id: Option<&str>,
        payload: &Value,
    ) -> Result<i64> {
        let conn = self.conn.lock().expect("db mutex");
        conn.execute(
            "INSERT INTO admin_events (kind, scope, session_key, entity_id, payload, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                kind,
                scope,
                session_key,
                entity_id,
                payload.to_string(),
                now_rfc3339()
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn latest_admin_sequence(&self) -> Result<i64> {
        let conn = self.conn.lock().expect("db mutex");
        conn.query_row(
            "SELECT COALESCE(MAX(sequence), 0) FROM admin_events",
            [],
            |row| row.get(0),
        )
        .context("admin sequence")
    }

    pub fn list_admin_events(&self, after: i64, limit: i64) -> Result<Vec<Value>> {
        let conn = self.conn.lock().expect("db mutex");
        let mut stmt = conn.prepare(
            "SELECT sequence, kind, scope, session_key, entity_id, payload, created_at FROM admin_events WHERE sequence > ?1 ORDER BY sequence ASC LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![after, limit], |row| {
                let payload: String = row.get(5)?;
                Ok(json!({
                    "sequence": row.get::<_, i64>(0)?,
                    "kind": row.get::<_, String>(1)?,
                    "scope": row.get::<_, String>(2)?,
                    "sessionKey": row.get::<_, Option<String>>(3)?,
                    "entityId": row.get::<_, Option<String>>(4)?,
                    "payload": serde_json::from_str::<Value>(&payload).unwrap_or(json!({})),
                    "createdAt": row.get::<_, String>(6)?,
                }))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn preflight(&self, operation: &str) -> Result<Value> {
        let conn = self.conn.lock().expect("db mutex");
        let mut job_stmt =
            conn.prepare("SELECT session_key, id FROM background_jobs WHERE status = 'running'")?;
        let running_jobs: Vec<(String, String)> = job_stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        drop(job_stmt);
        drop(conn);
        let mut impacts = Vec::new();
        for (session_key, job_id) in &running_jobs {
            impacts.push(json!({
                "type": "running_background_job",
                "sessionKey": session_key,
                "jobId": job_id
            }));
        }
        Ok(json!({
            "operation": operation,
            "safe": impacts.is_empty(),
            "requiresAllowActive": !impacts.is_empty(),
            "runningJobCount": running_jobs.len(),
            "impacts": impacts,
        }))
    }
}

fn map_session_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionRow> {
    Ok(SessionRow {
        key: row.get(0)?,
        id: row.get(1)?,
        channel_id: row.get(2)?,
        channel_name: row.get(3)?,
        channel_type: row.get(4)?,
        root_thread_ts: row.get(5)?,
        workspace_path: row.get(6)?,
        updated_at: row.get(7)?,
        created_at: row.get(8)?,
        profile_id: row.get(9)?,
        model: row.get(10)?,
        thinking: row.get(11)?,
        last_slack_reply_at: row.get(12)?,
        initiator_user_id: row.get(13)?,
        selection_blocked_at: row.get(14)?,
        selection_block_reason: row.get(15)?,
    })
}

fn map_job_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<JobRow> {
    Ok(JobRow {
        id: row.get(0)?,
        token: row.get(1)?,
        session_key: row.get(2)?,
        channel_id: row.get(3)?,
        root_thread_ts: row.get(4)?,
        kind: row.get(5)?,
        shell: row.get(6)?,
        cwd: row.get(7)?,
        script_path: row.get(8)?,
        restart_on_boot: row.get::<_, i64>(9)? != 0,
        status: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn ensure_session_is_idempotent() {
        let dir = tempdir().unwrap();
        let db = GatewayDb::open(dir.path(), &dir.path().join("workspaces")).unwrap();
        let first = db
            .ensure_session("C1", "1.0", Some("channel"), Some("U1"), Some("1.1"))
            .unwrap();
        let second = db
            .ensure_session("C1", "1.0", Some("channel"), None, None)
            .unwrap();
        assert_eq!(first.key, "C1:1.0");
        assert_eq!(first.key, second.key);
        assert_eq!(first.id, second.id);
    }
}
