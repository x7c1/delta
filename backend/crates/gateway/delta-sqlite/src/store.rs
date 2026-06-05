//! [`SqliteStore`]: the concrete [`SessionStore`].

use async_trait::async_trait;
use rusqlite::{params, Connection, OptionalExtension, Row};
use tokio::sync::Mutex;

use delta_model::{
    ContentBlock, Message, MessageUuid, PendingSend, PendingSendStatus, PermissionRequest,
    PermissionStatus, PromptId, Role, Session, SessionId, SessionStatus, Thread, ThreadId,
};
use delta_usecase::{NewSession, SessionStore};

use crate::error::{Error, Result};
use crate::schema::SCHEMA_SQL;
use crate::time::now_iso8601;

/// The trunk thread title. The first registered session always has one.
const MAIN_THREAD_TITLE: &str = "main";

/// A SQLite-backed session store.
pub struct SqliteStore {
    conn: Mutex<Connection>,
}

impl SqliteStore {
    /// Open (or create) a store at `path`, applying the schema migration.
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    /// Open an in-memory store (used by tests).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "foreign_keys", true)?;
        conn.execute_batch(SCHEMA_SQL)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }
}

fn map_session(row: &Row<'_>) -> rusqlite::Result<(SessionId, String, String, Option<String>, String, String)>
{
    Ok((
        SessionId::from(row.get::<_, String>(0)?),
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
    ))
}

fn session_from_parts(
    id: SessionId,
    cwd: String,
    transcript_path: String,
    title: Option<String>,
    status: String,
    created_at: String,
) -> Result<Session> {
    Ok(Session {
        id,
        cwd,
        transcript_path,
        title,
        status: SessionStatus::parse(&status)?,
        created_at,
    })
}

fn thread_from_row(row: &Row<'_>) -> Result<Thread> {
    Ok(Thread {
        id: ThreadId(row.get(0)?),
        session_id: SessionId::from(row.get::<_, String>(1)?),
        title: row.get(2)?,
        parent_thread_id: row.get::<_, Option<i64>>(3)?.map(ThreadId),
        root_message_uuid: row.get::<_, Option<String>>(4)?.map(MessageUuid::from),
        created_at: row.get(5)?,
    })
}

fn pending_send_from_row(row: &Row<'_>) -> Result<PendingSend> {
    Ok(PendingSend {
        id: row.get(0)?,
        session_id: SessionId::from(row.get::<_, String>(1)?),
        thread_id: ThreadId(row.get(2)?),
        semantic_parent_uuid: row.get::<_, Option<String>>(3)?.map(MessageUuid::from),
        text: row.get(4)?,
        locator_quote: row.get(5)?,
        status: PendingSendStatus::parse(&row.get::<_, String>(6)?)?,
        matched_uuid: row.get::<_, Option<String>>(7)?.map(MessageUuid::from),
        created_at: row.get(8)?,
    })
}

fn message_from_row(row: &Row<'_>) -> Result<Message> {
    let content_json: Option<String> = row.get(9)?;
    let content: Vec<ContentBlock> = match content_json {
        Some(json) => serde_json::from_str(&json).unwrap_or_default(),
        None => Vec::new(),
    };
    Ok(Message {
        uuid: MessageUuid::from(row.get::<_, String>(0)?),
        session_id: SessionId::from(row.get::<_, String>(1)?),
        thread_id: ThreadId(row.get(2)?),
        role: Role::parse(&row.get::<_, String>(3)?)?,
        linear_parent_uuid: row.get::<_, Option<String>>(4)?.map(MessageUuid::from),
        semantic_parent_uuid: row.get::<_, Option<String>>(5)?.map(MessageUuid::from),
        prompt_id: row.get::<_, Option<String>>(6)?.map(PromptId::from),
        seq: row.get(7)?,
        content_text: row.get(8)?,
        content,
        created_at: row.get(10)?,
    })
}

const SESSION_COLS: &str = "id, cwd, transcript_path, title, status, created_at";
const THREAD_COLS: &str = "id, session_id, title, parent_thread_id, root_message_uuid, created_at";
const PENDING_COLS: &str =
    "id, session_id, thread_id, semantic_parent_uuid, text, locator_quote, status, matched_uuid, created_at";
const MESSAGE_COLS: &str = "uuid, session_id, thread_id, role, linear_parent_uuid, semantic_parent_uuid, prompt_id, seq, content_text, content_json, created_at";

#[async_trait]
impl SessionStore for SqliteStore {
    async fn register_session(
        &self,
        new: NewSession,
    ) -> std::result::Result<(Session, ThreadId), delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let now = now_iso8601();

        // Insert the session only if absent.
        conn.execute(
            "INSERT OR IGNORE INTO session (id, cwd, transcript_path, title, status, created_at)
             VALUES (?1, ?2, ?3, NULL, 'active', ?4)",
            params![new.id.as_str(), new.cwd, new.transcript_path, now],
        )
        .map_err(Error::from)?;

        let session = conn
            .query_row(
                &format!("SELECT {SESSION_COLS} FROM session WHERE id = ?1"),
                params![new.id.as_str()],
                map_session,
            )
            .map_err(Error::from)
            .and_then(|p| session_from_parts(p.0, p.1, p.2, p.3, p.4, p.5))?;

        // Ensure a main thread exists.
        let main_id: Option<i64> = conn
            .query_row(
                "SELECT id FROM thread WHERE session_id = ?1 AND title = ?2 ORDER BY id LIMIT 1",
                params![new.id.as_str(), MAIN_THREAD_TITLE],
                |r| r.get(0),
            )
            .optional()
            .map_err(Error::from)?;

        let main_id = match main_id {
            Some(id) => id,
            None => {
                conn.execute(
                    "INSERT INTO thread (session_id, title, parent_thread_id, root_message_uuid, created_at)
                     VALUES (?1, ?2, NULL, NULL, ?3)",
                    params![new.id.as_str(), MAIN_THREAD_TITLE, now],
                )
                .map_err(Error::from)?;
                conn.last_insert_rowid()
            }
        };

        Ok((session, ThreadId(main_id)))
    }

    async fn current_session(
        &self,
    ) -> std::result::Result<Option<Session>, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let parts = conn
            .query_row(
                &format!("SELECT {SESSION_COLS} FROM session ORDER BY created_at LIMIT 1"),
                [],
                map_session,
            )
            .optional()
            .map_err(Error::from)?;
        match parts {
            Some(p) => Ok(Some(session_from_parts(p.0, p.1, p.2, p.3, p.4, p.5)?)),
            None => Ok(None),
        }
    }

    async fn main_thread_id(
        &self,
        session_id: &SessionId,
    ) -> std::result::Result<ThreadId, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let id: i64 = conn
            .query_row(
                "SELECT id FROM thread WHERE session_id = ?1 AND title = ?2 ORDER BY id LIMIT 1",
                params![session_id.as_str(), MAIN_THREAD_TITLE],
                |r| r.get(0),
            )
            .map_err(Error::from)?;
        Ok(ThreadId(id))
    }

    async fn thread(
        &self,
        id: ThreadId,
    ) -> std::result::Result<Option<Thread>, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let row = conn
            .query_row(
                &format!("SELECT {THREAD_COLS} FROM thread WHERE id = ?1"),
                params![id.value()],
                |r| Ok(thread_from_row(r)),
            )
            .optional()
            .map_err(Error::from)?;
        match row {
            Some(thread) => Ok(Some(thread?)),
            None => Ok(None),
        }
    }

    async fn list_threads(
        &self,
        session_id: &SessionId,
    ) -> std::result::Result<Vec<Thread>, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {THREAD_COLS} FROM thread WHERE session_id = ?1 ORDER BY id"
            ))
            .map_err(Error::from)?;
        let rows = stmt
            .query_map(params![session_id.as_str()], |r| Ok(thread_from_row(r)))
            .map_err(Error::from)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(Error::from)??);
        }
        Ok(out)
    }

    async fn create_thread(
        &self,
        session_id: &SessionId,
        title: &str,
        parent_thread_id: Option<ThreadId>,
        root_message_uuid: Option<&MessageUuid>,
    ) -> std::result::Result<Thread, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let now = now_iso8601();
        conn.execute(
            "INSERT INTO thread (session_id, title, parent_thread_id, root_message_uuid, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                session_id.as_str(),
                title,
                parent_thread_id.map(ThreadId::value),
                root_message_uuid.map(MessageUuid::as_str),
                now,
            ],
        )
        .map_err(Error::from)?;
        let id = conn.last_insert_rowid();
        Ok(Thread {
            id: ThreadId(id),
            session_id: session_id.clone(),
            title: title.to_owned(),
            parent_thread_id,
            root_message_uuid: root_message_uuid.cloned(),
            created_at: now,
        })
    }

    async fn enqueue_send(
        &self,
        session_id: &SessionId,
        thread_id: ThreadId,
        semantic_parent_uuid: Option<&MessageUuid>,
        text: &str,
        locator_quote: Option<&str>,
    ) -> std::result::Result<PendingSend, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let now = now_iso8601();
        conn.execute(
            "INSERT INTO pending_send
             (session_id, thread_id, semantic_parent_uuid, text, locator_quote, status, matched_uuid, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'pending', NULL, ?6)",
            params![
                session_id.as_str(),
                thread_id.value(),
                semantic_parent_uuid.map(MessageUuid::as_str),
                text,
                locator_quote,
                now,
            ],
        )
        .map_err(Error::from)?;
        let id = conn.last_insert_rowid();
        Ok(PendingSend {
            id,
            session_id: session_id.clone(),
            thread_id,
            semantic_parent_uuid: semantic_parent_uuid.cloned(),
            text: text.to_owned(),
            locator_quote: locator_quote.map(str::to_owned),
            status: PendingSendStatus::Pending,
            matched_uuid: None,
            created_at: now,
        })
    }

    async fn head_pending_send(
        &self,
        session_id: &SessionId,
    ) -> std::result::Result<Option<PendingSend>, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let row = conn
            .query_row(
                &format!(
                    "SELECT {PENDING_COLS} FROM pending_send
                     WHERE session_id = ?1 AND status = 'pending'
                     ORDER BY id LIMIT 1"
                ),
                params![session_id.as_str()],
                |r| Ok(pending_send_from_row(r)),
            )
            .optional()
            .map_err(Error::from)?;
        match row {
            Some(send) => Ok(Some(send?)),
            None => Ok(None),
        }
    }

    async fn mark_send_matched(
        &self,
        id: i64,
        matched_uuid: &MessageUuid,
    ) -> std::result::Result<(), delta_usecase::Error> {
        let conn = self.conn.lock().await;
        conn.execute(
            "UPDATE pending_send SET status = 'matched', matched_uuid = ?2 WHERE id = ?1",
            params![id, matched_uuid.as_str()],
        )
        .map_err(Error::from)?;
        Ok(())
    }

    async fn upsert_messages(
        &self,
        messages: &[Message],
    ) -> std::result::Result<(), delta_usecase::Error> {
        let mut conn = self.conn.lock().await;
        let tx = conn.transaction().map_err(Error::from)?;
        for m in messages {
            let content_json = serde_json::to_string(&m.content).unwrap_or_else(|_| "[]".into());
            // The API contract promises an ISO-8601 `created_at`. A transcript
            // line may omit its timestamp, which surfaces here as an empty
            // string; fall back to the ingest time so the stored (and served)
            // value is always a valid timestamp rather than `""`.
            let created_at = if m.created_at.is_empty() {
                now_iso8601()
            } else {
                m.created_at.clone()
            };
            tx.execute(
                &format!(
                    "INSERT INTO message ({MESSAGE_COLS}) VALUES
                     (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                     ON CONFLICT(uuid) DO UPDATE SET
                       thread_id = excluded.thread_id,
                       role = excluded.role,
                       linear_parent_uuid = excluded.linear_parent_uuid,
                       semantic_parent_uuid = excluded.semantic_parent_uuid,
                       prompt_id = excluded.prompt_id,
                       seq = excluded.seq,
                       content_text = excluded.content_text,
                       content_json = excluded.content_json"
                ),
                params![
                    m.uuid.as_str(),
                    m.session_id.as_str(),
                    m.thread_id.value(),
                    m.role.as_str(),
                    m.linear_parent_uuid.as_ref().map(MessageUuid::as_str),
                    m.semantic_parent_uuid.as_ref().map(MessageUuid::as_str),
                    m.prompt_id.as_ref().map(PromptId::as_str),
                    m.seq,
                    m.content_text,
                    content_json,
                    created_at,
                ],
            )
            .map_err(Error::from)?;
        }
        tx.commit().map_err(Error::from)?;
        Ok(())
    }

    async fn message_count(
        &self,
        session_id: &SessionId,
    ) -> std::result::Result<usize, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM message WHERE session_id = ?1",
                params![session_id.as_str()],
                |r| r.get(0),
            )
            .map_err(Error::from)?;
        Ok(count as usize)
    }

    async fn thread_messages(
        &self,
        thread_id: ThreadId,
    ) -> std::result::Result<Vec<Message>, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {MESSAGE_COLS} FROM message WHERE thread_id = ?1 ORDER BY seq"
            ))
            .map_err(Error::from)?;
        let rows = stmt
            .query_map(params![thread_id.value()], |r| Ok(message_from_row(r)))
            .map_err(Error::from)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(Error::from)??);
        }
        Ok(out)
    }

    async fn record_permission_request(
        &self,
        session_id: &SessionId,
        tool_name: &str,
        tool_input_json: &str,
    ) -> std::result::Result<PermissionRequest, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let now = now_iso8601();
        conn.execute(
            "INSERT INTO permission_request
             (session_id, tool_name, tool_input_json, status, decision_reason, created_at, decided_at)
             VALUES (?1, ?2, ?3, 'pending', NULL, ?4, NULL)",
            params![session_id.as_str(), tool_name, tool_input_json, now],
        )
        .map_err(Error::from)?;
        let id = conn.last_insert_rowid();
        Ok(PermissionRequest {
            id,
            session_id: session_id.clone(),
            tool_name: tool_name.to_owned(),
            tool_input_json: tool_input_json.to_owned(),
            status: PermissionStatus::Pending,
            decision_reason: None,
            created_at: now,
            decided_at: None,
        })
    }
}

#[cfg(test)]
mod tests;
