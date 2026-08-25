//! The send FIFO: enqueue, dispatch, restore, and match transitions.

use rusqlite::{params, OptionalExtension, Row};

use delta_model::{MessageUuid, Send, SendStatus, SessionId, ThreadId};

use crate::error::{Error, Result};
use crate::time::now_iso8601;

use super::SqliteStore;

fn send_from_row(row: &Row<'_>) -> Result<Send> {
    Ok(Send {
        id: row.get(0)?,
        session_id: SessionId::from(row.get::<_, String>(1)?),
        thread_id: ThreadId(row.get(2)?),
        semantic_parent_uuid: row.get::<_, Option<String>>(3)?.map(MessageUuid::from),
        text: row.get(4)?,
        locator_quote: row.get(5)?,
        status: SendStatus::parse(&row.get::<_, String>(6)?)?,
        matched_uuid: row.get::<_, Option<String>>(7)?.map(MessageUuid::from),
        created_at: row.get(8)?,
        restored_at: row.get(9)?,
    })
}

const SEND_COLS: &str =
    "id, session_id, thread_id, semantic_parent_uuid, text, locator_quote, status, matched_uuid, created_at, restored_at";

impl SqliteStore {
    pub(super) async fn enqueue_send(
        &self,
        session_id: &SessionId,
        thread_id: ThreadId,
        semantic_parent_uuid: Option<&MessageUuid>,
        text: &str,
        locator_quote: Option<&str>,
    ) -> std::result::Result<Send, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let now = now_iso8601();
        conn.execute(
            "INSERT INTO send
             (session_id, thread_id, semantic_parent_uuid, text, locator_quote, status, matched_uuid, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'dispatched', NULL, ?6)",
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
        Ok(Send {
            id,
            session_id: session_id.clone(),
            thread_id,
            semantic_parent_uuid: semantic_parent_uuid.cloned(),
            text: text.to_owned(),
            locator_quote: locator_quote.map(str::to_owned),
            status: SendStatus::Dispatched,
            matched_uuid: None,
            created_at: now,
            restored_at: None,
        })
    }

    pub(super) async fn enqueue_queued_send(
        &self,
        session_id: &SessionId,
        thread_id: ThreadId,
        semantic_parent_uuid: Option<&MessageUuid>,
        text: &str,
        locator_quote: Option<&str>,
    ) -> std::result::Result<Send, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let now = now_iso8601();
        conn.execute(
            "INSERT INTO send
             (session_id, thread_id, semantic_parent_uuid, text, locator_quote, status, matched_uuid, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'queued', NULL, ?6)",
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
        Ok(Send {
            id,
            session_id: session_id.clone(),
            thread_id,
            semantic_parent_uuid: semantic_parent_uuid.cloned(),
            text: text.to_owned(),
            locator_quote: locator_quote.map(str::to_owned),
            status: SendStatus::Queued,
            matched_uuid: None,
            created_at: now,
            restored_at: None,
        })
    }

    pub(super) async fn send(
        &self,
        id: i64,
    ) -> std::result::Result<Option<Send>, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let row = conn
            .query_row(
                &format!("SELECT {SEND_COLS} FROM send WHERE id = ?1"),
                params![id],
                |r| Ok(send_from_row(r)),
            )
            .optional()
            .map_err(Error::from)?;
        row.transpose().map_err(Into::into)
    }

    pub(super) async fn next_queued_send(
        &self,
        session_id: &SessionId,
    ) -> std::result::Result<Option<Send>, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let row = conn
            .query_row(
                &format!(
                    "SELECT {SEND_COLS} FROM send
                     WHERE session_id = ?1 AND status = 'queued'
                       AND restored_at IS NULL
                     ORDER BY id LIMIT 1"
                ),
                params![session_id.as_str()],
                |r| Ok(send_from_row(r)),
            )
            .optional()
            .map_err(Error::from)?;
        row.transpose().map_err(Into::into)
    }

    pub(super) async fn open_sends(
        &self,
        session_id: &SessionId,
    ) -> std::result::Result<Vec<Send>, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {SEND_COLS} FROM send
                 WHERE session_id = ?1 AND status IN ('queued', 'dispatched')
                 ORDER BY id"
            ))
            .map_err(Error::from)?;
        let rows = stmt
            .query_map(params![session_id.as_str()], |r| Ok(send_from_row(r)))
            .map_err(Error::from)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(Error::from)??);
        }
        Ok(out)
    }

    pub(super) async fn promote_queued_send(
        &self,
        id: i64,
    ) -> std::result::Result<(), delta_usecase::Error> {
        let conn = self.conn.lock().await;
        conn.execute(
            "UPDATE send SET status = 'dispatched' WHERE id = ?1 AND status = 'queued'",
            params![id],
        )
        .map_err(Error::from)?;
        Ok(())
    }

    pub(super) async fn requeue_send(
        &self,
        id: i64,
    ) -> std::result::Result<(), delta_usecase::Error> {
        let conn = self.conn.lock().await;
        conn.execute(
            "UPDATE send SET status = 'queued' WHERE id = ?1 AND status = 'dispatched'",
            params![id],
        )
        .map_err(Error::from)?;
        Ok(())
    }

    pub(super) async fn restore_all_dispatched(
        &self,
    ) -> std::result::Result<usize, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let now = now_iso8601();
        let affected = conn
            .execute(
                "UPDATE send SET status = 'queued', restored_at = ?1
                 WHERE status = 'dispatched'",
                params![now],
            )
            .map_err(Error::from)?;
        Ok(affected)
    }

    pub(super) async fn release_restored_send(
        &self,
        id: i64,
    ) -> std::result::Result<bool, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let affected = conn
            .execute(
                "UPDATE send SET restored_at = NULL
                 WHERE id = ?1 AND status = 'queued' AND restored_at IS NOT NULL",
                params![id],
            )
            .map_err(Error::from)?;
        Ok(affected > 0)
    }

    pub(super) async fn head_dispatched_send(
        &self,
        session_id: &SessionId,
    ) -> std::result::Result<Option<Send>, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let row = conn
            .query_row(
                &format!(
                    "SELECT {SEND_COLS} FROM send
                     WHERE session_id = ?1 AND status = 'dispatched'
                     ORDER BY id LIMIT 1"
                ),
                params![session_id.as_str()],
                |r| Ok(send_from_row(r)),
            )
            .optional()
            .map_err(Error::from)?;
        match row {
            Some(send) => Ok(Some(send?)),
            None => Ok(None),
        }
    }

    pub(super) async fn dispatched_sends(
        &self,
        session_id: &SessionId,
    ) -> std::result::Result<Vec<Send>, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {SEND_COLS} FROM send
                 WHERE session_id = ?1 AND status = 'dispatched'
                 ORDER BY id"
            ))
            .map_err(Error::from)?;
        let rows = stmt
            .query_map(params![session_id.as_str()], |r| Ok(send_from_row(r)))
            .map_err(Error::from)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(Error::from)??);
        }
        Ok(out)
    }

    pub(super) async fn mark_send_matched(
        &self,
        id: i64,
        matched_uuid: &MessageUuid,
    ) -> std::result::Result<(), delta_usecase::Error> {
        let conn = self.conn.lock().await;
        conn.execute(
            "UPDATE send SET status = 'matched', matched_uuid = ?2 WHERE id = ?1",
            params![id, matched_uuid.as_str()],
        )
        .map_err(Error::from)?;
        Ok(())
    }

    /// Settle a still-`dispatched` send as delivered, leaving `matched_uuid`
    /// `NULL` because no transcript line claimed it. The status guard keeps a
    /// row that already matched, was cancelled, or was parked untouched — see
    /// the port docs for why "delivered but unattributed" is `matched` rather
    /// than `cancelled`.
    pub(super) async fn settle_send_delivered(
        &self,
        id: i64,
    ) -> std::result::Result<bool, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let affected = conn
            .execute(
                "UPDATE send SET status = 'matched' WHERE id = ?1 AND status = 'dispatched'",
                params![id],
            )
            .map_err(Error::from)?;
        Ok(affected > 0)
    }

    pub(super) async fn cancel_send(
        &self,
        id: i64,
    ) -> std::result::Result<(), delta_usecase::Error> {
        let conn = self.conn.lock().await;
        conn.execute(
            "UPDATE send SET status = 'cancelled' WHERE id = ?1",
            params![id],
        )
        .map_err(Error::from)?;
        Ok(())
    }

    pub(super) async fn cancel_queued_send(
        &self,
        id: i64,
    ) -> std::result::Result<bool, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let affected = conn
            .execute(
                "UPDATE send SET status = 'cancelled' WHERE id = ?1 AND status = 'queued'",
                params![id],
            )
            .map_err(Error::from)?;
        Ok(affected > 0)
    }
}
