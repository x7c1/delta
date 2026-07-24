//! Thread rows: the main thread, branch threads, and thread lookups.

use rusqlite::{params, OptionalExtension, Row};

use delta_model::{MessageUuid, SessionId, Thread, ThreadId};

use crate::error::{Error, Result};
use crate::time::now_iso8601;

use super::{SqliteStore, MAIN_THREAD_TITLE};

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

/// Thread columns plus the derived `root_message_uuid`: the branch edge's
/// canonical home is `message.semantic_parent_uuid`, so the root is computed
/// from the thread's first semantically parented message — falling back to the
/// thread's earliest semantically parented send for the window between the
/// branch send being recorded and its user line being ingested. Requires the
/// query to select `FROM thread` (both thread queries do).
const THREAD_COLS: &str = "id, session_id, title, parent_thread_id, \
     COALESCE( \
       (SELECT m.semantic_parent_uuid FROM message m \
         WHERE m.thread_id = thread.id AND m.semantic_parent_uuid IS NOT NULL \
         ORDER BY m.seq LIMIT 1), \
       (SELECT s.semantic_parent_uuid FROM send s \
         WHERE s.thread_id = thread.id AND s.semantic_parent_uuid IS NOT NULL \
         ORDER BY s.id LIMIT 1) \
     ) AS root_message_uuid, created_at";

impl SqliteStore {
    pub(super) async fn main_thread_id(
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

    pub(super) async fn thread(
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

    pub(super) async fn list_threads(
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

    pub(super) async fn create_thread(
        &self,
        session_id: &SessionId,
        title: &str,
        parent_thread_id: Option<ThreadId>,
    ) -> std::result::Result<Thread, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let now = now_iso8601();
        conn.execute(
            "INSERT INTO thread (session_id, title, parent_thread_id, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                session_id.as_str(),
                title,
                parent_thread_id.map(ThreadId::value),
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
            // Derived from the thread's branch send/message, neither of which
            // exists yet at creation time.
            root_message_uuid: None,
            created_at: now,
        })
    }

    pub(super) async fn latest_user_thread(
        &self,
        session_id: &SessionId,
    ) -> std::result::Result<Option<ThreadId>, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let id: Option<i64> = conn
            .query_row(
                "SELECT thread_id FROM message
                 WHERE session_id = ?1 AND role = 'user'
                 ORDER BY seq DESC LIMIT 1",
                params![session_id.as_str()],
                |r| r.get(0),
            )
            .optional()
            .map_err(Error::from)?;
        Ok(id.map(ThreadId))
    }
}
