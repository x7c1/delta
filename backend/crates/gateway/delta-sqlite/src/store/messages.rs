//! The message cache: transcript upserts, the sync cursor, and reads.

use rusqlite::{params, OptionalExtension, Row};

use delta_model::{Message, MessageUuid, PromptId, Role, SessionId, ThreadId};

use crate::content_record::{decode_content, encode_content};
use crate::error::{Error, Result};

use super::SqliteStore;

fn message_from_row(row: &Row<'_>) -> Result<Message> {
    let content_json: Option<String> = row.get(9)?;
    let content = match content_json {
        Some(json) => decode_content(&json),
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
        model: row.get(11)?,
        git_branch: row.get(12)?,
        cwd: row.get(13)?,
        response_time_ms: row.get(14)?,
        provider_item_id: row.get(15)?,
    })
}

const MESSAGE_COLS: &str = "uuid, session_id, thread_id, role, linear_parent_uuid, semantic_parent_uuid, prompt_id, seq, content_text, content_json, created_at, model, git_branch, cwd, response_time_ms, provider_item_id";

impl SqliteStore {
    pub(super) async fn upsert_messages(
        &self,
        messages: &[Message],
    ) -> std::result::Result<(), delta_usecase::Error> {
        let mut conn = self.conn.lock().await;
        let tx = conn.transaction().map_err(Error::from)?;
        // Sessions whose messages this batch touched: their denormalized
        // `last_activity_at` is recomputed once after the inserts (below).
        // Collected as the distinct session ids in the batch, in first-seen
        // order, so the recompute runs once per session regardless of how many
        // of its messages are in the batch.
        let mut touched: Vec<&SessionId> = Vec::new();
        for m in messages {
            if !touched.iter().any(|id| **id == m.session_id) {
                touched.push(&m.session_id);
            }
        }
        for m in messages {
            let content_json = encode_content(&m.content);
            tx.execute(
                &format!(
                    // `thread_id` and `semantic_parent_uuid` form the thread
                    // overlay: they are authoritative once assigned on the
                    // FIRST ingest of a line and must NOT be overwritten on a
                    // re-ingest. Branch attribution only ever happens on the
                    // first ingest, because the send row is recorded before
                    // the keystrokes are dispatched, so by the time the user
                    // line appears in the transcript its send is still
                    // `dispatched` and the prompt-echo correlation attaches it
                    // to the branch thread. A second ingest of the same line (e.g.
                    // hook-sync racing the background tail, or a re-sync) finds
                    // the send already `matched`, so `sync_transcript` falls
                    // back to the external-input branch and recomputes
                    // `(thread_id, semantic_parent) = (main, None)`. Overwriting
                    // here would clobber the correct branch attribution back to
                    // main and leave branch threads empty. The remaining columns
                    // are transcript-derived cache and may keep refreshing.
                    // `created_at` may be NULL: a transcript line without a
                    // timestamp is stored as such, never as a sentinel.
                    "INSERT INTO message ({MESSAGE_COLS}) VALUES
                     (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
                     ON CONFLICT(session_id, uuid) DO UPDATE SET
                       role = excluded.role,
                       linear_parent_uuid = excluded.linear_parent_uuid,
                       prompt_id = excluded.prompt_id,
                       seq = excluded.seq,
                       content_text = excluded.content_text,
                       content_json = excluded.content_json,
                       created_at = excluded.created_at,
                       model = excluded.model,
                       git_branch = excluded.git_branch,
                       cwd = excluded.cwd,
                       response_time_ms = excluded.response_time_ms,
                       provider_item_id = excluded.provider_item_id"
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
                    m.created_at,
                    m.model,
                    m.git_branch,
                    m.cwd,
                    m.response_time_ms,
                    m.provider_item_id,
                ],
            )
            .map_err(Error::from)?;
        }
        // Refresh the denormalized recency for every session this batch touched,
        // recomputing `MAX(message.created_at)` once per session (a single-session
        // lookup backed by `ix_message_session_created`). Recomputing — rather
        // than taking the batch max — keeps the column correct even when a
        // re-ingest rewrites a message's `created_at`, and yields NULL for a
        // session whose only messages have no timestamp. The whole thing is in
        // the same transaction as the inserts, so the column can never lag the
        // rows it summarizes.
        for session_id in touched {
            tx.execute(
                "UPDATE session SET last_activity_at = \
                   (SELECT MAX(created_at) FROM message WHERE session_id = ?1) \
                 WHERE id = ?1",
                params![session_id.as_str()],
            )
            .map_err(Error::from)?;
        }
        tx.commit().map_err(Error::from)?;
        Ok(())
    }

    pub(super) async fn last_activity_at(
        &self,
        session_id: &SessionId,
    ) -> std::result::Result<Option<String>, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        // Read the denormalized column directly: it is the maintained
        // `MAX(message.created_at)` (NULL when the session has no timestamped
        // message), kept current by `upsert_messages`.
        let latest: Option<String> = conn
            .query_row(
                "SELECT last_activity_at FROM session WHERE id = ?1",
                params![session_id.as_str()],
                |r| r.get(0),
            )
            .map_err(Error::from)?;
        Ok(latest)
    }

    pub(super) async fn message_count(
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

    pub(super) async fn transcript_lines_read(
        &self,
        session_id: &SessionId,
    ) -> std::result::Result<usize, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        // The cursor lives in `sync_cursor`, written lazily on the first sync:
        // a session with no cursor row simply has not been synced yet, so it
        // reads as zero.
        let lines: Option<i64> = conn
            .query_row(
                "SELECT lines_read FROM sync_cursor WHERE session_id = ?1",
                params![session_id.as_str()],
                |r| r.get(0),
            )
            .optional()
            .map_err(Error::from)?;
        Ok(lines.unwrap_or(0) as usize)
    }

    pub(super) async fn set_transcript_lines_read(
        &self,
        session_id: &SessionId,
        lines: usize,
    ) -> std::result::Result<(), delta_usecase::Error> {
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT INTO sync_cursor (session_id, lines_read) VALUES (?1, ?2)
             ON CONFLICT(session_id) DO UPDATE SET lines_read = excluded.lines_read",
            params![session_id.as_str(), lines as i64],
        )
        .map_err(Error::from)?;
        Ok(())
    }

    pub(super) async fn thread_messages(
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
}
