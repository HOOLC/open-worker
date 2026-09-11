//! Frozen caller-side commands and attachment bytes survive lost Mesh replies.
use super::*;
use base64::Engine;

pub(super) fn initialize(conn: &Connection) -> Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS chat_outgoing(
        request_key TEXT PRIMARY KEY REFERENCES chat_receipts(request_key), value TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS chat_prepared_files(
        request_key TEXT NOT NULL REFERENCES chat_receipts(request_key), target TEXT NOT NULL,
        file_id TEXT NOT NULL, reference TEXT NOT NULL, media_type TEXT NOT NULL, content BLOB NOT NULL,
        PRIMARY KEY(request_key,file_id));")?;
    conn.execute_batch("CREATE TABLE IF NOT EXISTS chat_file_metadata(artifact_id TEXT PRIMARY KEY,reference TEXT NOT NULL);")?;
    conn.execute_batch("CREATE TABLE IF NOT EXISTS chat_agent_home(agent_id TEXT PRIMARY KEY REFERENCES node_agents(id),chat_id TEXT NOT NULL REFERENCES chat_channels(chat_id));
        CREATE TABLE IF NOT EXISTS chat_receiving_policy(agent_id TEXT NOT NULL,target TEXT NOT NULL,chat_id TEXT NOT NULL,generation INTEGER NOT NULL,subscribed INTEGER NOT NULL,PRIMARY KEY(agent_id,target,chat_id));
        CREATE TABLE IF NOT EXISTS chat_direct_inputs(sequence INTEGER PRIMARY KEY AUTOINCREMENT,
        request_key TEXT NOT NULL UNIQUE,agent_id TEXT NOT NULL,author TEXT NOT NULL,content TEXT NOT NULL,delivered INTEGER NOT NULL DEFAULT 0);
        CREATE INDEX IF NOT EXISTS chat_direct_pending ON chat_direct_inputs(agent_id,delivered,sequence);")?;
    Ok(())
}

impl GatewayDb {
    pub fn chat_file_ref(&self, chat: &str, id: &str) -> Result<zork_client_types::files::FileRef> {
        let channel = self.chat(chat)?;
        let cached: Option<String> = {
            let conn = self.conn.lock().expect("db mutex");
            let owned:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM conversation_artifacts WHERE artifact_id=?1 AND session_key=?2 UNION ALL SELECT 1 FROM task_artifacts a JOIN product_tasks t ON t.task_id=a.task_id WHERE a.artifact_id=?1 AND t.session_key=?2)",params![id,channel.session_key],|r|r.get(0))?;
            anyhow::ensure!(owned, "attachment_not_in_chat");
            conn.query_row(
                "SELECT reference FROM chat_file_metadata WHERE artifact_id=?1",
                [id],
                |r| r.get(0),
            )
            .optional()?
        };
        if let Some(value) = cached {
            return Ok(serde_json::from_str(&value)?);
        }
        let reference = self.conversation_file_ref(&channel.session_key, id)?;
        self.conn.lock().expect("db mutex").execute(
            "INSERT OR IGNORE INTO chat_file_metadata VALUES(?1,?2)",
            params![id, serde_json::to_string(&reference)?],
        )?;
        Ok(reference)
    }

    pub fn published_chat_chunk(&self, chat: &str, id: &str, offset: usize) -> Result<Value> {
        let reference = self.chat_file_ref(chat, id)?;
        anyhow::ensure!(offset <= reference.byte_len, "invalid_attachment_offset");
        let channel = self.chat(chat)?;
        let conn = self.conn.lock().expect("db mutex");
        let (media,content):(String,Vec<u8>)=conn.query_row("SELECT media_type,substr(content,?3+1,24576) FROM conversation_artifacts WHERE session_key=?1 AND artifact_id=?2 UNION ALL SELECT a.media_type,substr(a.content,?3+1,24576) FROM task_artifacts a JOIN product_tasks t ON t.task_id=a.task_id WHERE t.session_key=?1 AND a.artifact_id=?2",params![channel.session_key,id,offset],|r|Ok((r.get(0)?,r.get(1)?))).context("attachment_unavailable")?;
        Ok(
            json!({"reference":reference,"media_type":media,"offset":offset,"base64":base64::engine::general_purpose::STANDARD.encode(&content),"next_offset":offset+content.len()}),
        )
    }

    pub fn chat_receipt(&self, key: &str) -> Result<Option<Value>> {
        let value: Option<Option<String>> = self
            .conn
            .lock()
            .expect("db mutex")
            .query_row(
                "SELECT result FROM chat_receipts WHERE request_key=?1",
                [key],
                |r| r.get(0),
            )
            .optional()?;
        value
            .flatten()
            .map(|v| serde_json::from_str(&v).map_err(Into::into))
            .transpose()
    }

    pub fn chat_outgoing(&self, key: &str) -> Result<Option<Value>> {
        let value: Option<String> = self
            .conn
            .lock()
            .expect("db mutex")
            .query_row(
                "SELECT value FROM chat_outgoing WHERE request_key=?1",
                [key],
                |r| r.get(0),
            )
            .optional()?;
        value
            .map(|v| serde_json::from_str(&v).map_err(Into::into))
            .transpose()
    }

    pub fn save_chat_outgoing(
        &self,
        key: &str,
        target: &str,
        value: &Value,
        files: &[PreparedFile],
    ) -> Result<()> {
        let mut conn = self.conn.lock().expect("db mutex");
        let tx = conn.transaction()?;
        command_active(&tx, key)?;
        let bytes: usize = files.iter().map(|f| f.content.len()).sum();
        let pending: usize = tx.query_row(
            "SELECT COALESCE(SUM(length(content)),0) FROM chat_prepared_files",
            [],
            |r| r.get(0),
        )?;
        anyhow::ensure!(
            pending + bytes <= 512 * 1024 * 1024,
            "chat_pending_attachment_limit"
        );
        tx.execute(
            "INSERT INTO chat_outgoing(request_key,value) VALUES (?1,?2)",
            params![key, serde_json::to_string(value)?],
        )?;
        for file in files {
            tx.execute(
                "INSERT INTO chat_prepared_files VALUES (?1,?2,?3,?4,?5,?6)",
                params![
                    key,
                    target,
                    file.reference.id,
                    serde_json::to_string(&file.reference)?,
                    file.media_type,
                    file.content
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn finish_chat_outgoing(&self, key: &str, result: &Value) -> Result<()> {
        let mut conn = self.conn.lock().expect("db mutex");
        let tx = conn.transaction()?;
        finish(&tx, key, result)?;
        tx.execute(
            "DELETE FROM chat_prepared_files WHERE request_key=?1",
            [key],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Read only the requested bounded BLOB slice. The selected destination is
    /// fixed before publication; another paired node cannot fetch draft files.
    pub fn prepared_chat_chunk(
        &self,
        key: &str,
        peer: &str,
        id: &str,
        offset: usize,
    ) -> Result<Value> {
        let conn = self.conn.lock().expect("db mutex");
        let (reference,media,content):(String,String,Vec<u8>)=conn.query_row(
            "SELECT reference,media_type,substr(content,?4+1,24576) FROM chat_prepared_files WHERE request_key=?1 AND target=?2 AND file_id=?3",
            params![key,peer,id,offset],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).context("attachment_unavailable")?;
        let reference: zork_client_types::files::FileRef = serde_json::from_str(&reference)?;
        anyhow::ensure!(offset <= reference.byte_len, "invalid_attachment_offset");
        Ok(
            json!({"reference":reference,"media_type":media,"offset":offset,"base64":base64::engine::general_purpose::STANDARD.encode(&content),"next_offset":offset+content.len()}),
        )
    }
}
