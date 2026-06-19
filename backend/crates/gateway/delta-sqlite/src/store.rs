//! [`SqliteStore`]: the concrete [`SessionStore`].

use std::collections::BTreeMap;

use async_trait::async_trait;
use rusqlite::{named_params, params, Connection, OptionalExtension, Row};
use tokio::sync::Mutex;

use delta_model::{
    LaunchOption, Message, MessageUuid, PermissionRequest, PermissionStatus, PromptId, Role, Send,
    SendStatus, Session, SessionId, SessionStatus, Thread, ThreadId,
};
use delta_usecase::{NewSession, RecentWorkdir, SessionPageCursor, SessionPageRow, SessionStore};

use crate::content_record::{decode_content, encode_content};
use crate::error::{Error, Result};
use crate::schema::{ADDITIVE_COLUMNS, BACKFILL_LAST_ACTIVITY_SQL, RECENCY_INDEX_SQL, SCHEMA_SQL};
use crate::time::now_iso8601;

/// The trunk thread title. The first registered session always has one.
const MAIN_THREAD_TITLE: &str = "main";

/// A SQLite-backed session store.
pub struct SqliteStore {
    conn: Mutex<Connection>,
}

impl SqliteStore {
    /// Open (or create) a store at `path`, creating the schema if absent.
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
        // WAL keeps readers unblocked during writes. The pragma reports the
        // resulting mode as a result row, so it must be read with `query_row`;
        // an in-memory database legitimately reports `memory` instead of
        // `wal`, so the returned value is informational, not asserted.
        let _mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
        conn.pragma_update(None, "foreign_keys", true)?;
        conn.execute_batch(SCHEMA_SQL)?;
        Self::apply_additive_columns(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Bring an existing database up to the current column set.
    ///
    /// `CREATE TABLE IF NOT EXISTS` never alters a table that already exists, so
    /// a column added to a shipped table is invisible to a database created
    /// before it. Each [`ADDITIVE_COLUMNS`] entry is applied as a guarded
    /// `ALTER TABLE ... ADD COLUMN` only when the column is genuinely absent
    /// (SQLite has no `ADD COLUMN IF NOT EXISTS`), so this is a no-op on a fresh
    /// database that already declared the column in `SCHEMA_SQL`. Adding
    /// `session.last_activity_at` is followed by a one-time backfill so existing
    /// sessions get a correct recency value instead of a stale NULL.
    fn apply_additive_columns(conn: &Connection) -> Result<()> {
        for col in ADDITIVE_COLUMNS {
            if !column_exists(conn, col.table, col.column)? {
                conn.execute_batch(col.add_column_sql)?;
                if col.table == "session" && col.column == "last_activity_at" {
                    conn.execute_batch(BACKFILL_LAST_ACTIVITY_SQL)?;
                }
            }
        }
        // Created here rather than in `SCHEMA_SQL` because it references
        // `last_activity_at`, which an existing database only gains from the
        // guarded `ALTER TABLE` above. Idempotent (`IF NOT EXISTS`), so a fresh
        // database — where the column came from `SCHEMA_SQL` — creates it once
        // and a re-open is a no-op.
        conn.execute_batch(RECENCY_INDEX_SQL)?;
        Ok(())
    }
}

/// Whether `table` already has a column named `column`, via `PRAGMA table_info`.
fn column_exists(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    // `table` is a hard-coded schema identifier (never user input), so the
    // pragma-call form is safe; bound parameters are not accepted inside a
    // `PRAGMA table_info(...)` call.
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        if name == column {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The raw `session` columns of one row, in `SESSION_COLS` order, before the
/// status string is parsed into a domain [`Session`].
struct SessionParts {
    id: SessionId,
    cwd: String,
    transcript_path: Option<String>,
    title: Option<String>,
    status: String,
    created_at: String,
}

fn map_session(row: &Row<'_>) -> rusqlite::Result<SessionParts> {
    Ok(SessionParts {
        id: SessionId::from(row.get::<_, String>(0)?),
        cwd: row.get(1)?,
        transcript_path: row.get(2)?,
        title: row.get(3)?,
        status: row.get(4)?,
        created_at: row.get(5)?,
    })
}

fn session_from_parts(parts: SessionParts) -> Result<Session> {
    Ok(Session {
        id: parts.id,
        cwd: parts.cwd,
        transcript_path: parts.transcript_path,
        title: parts.title,
        status: SessionStatus::parse(&parts.status)?,
        created_at: parts.created_at,
    })
}

/// Map a session-list page row: the session columns followed by the stored
/// `last_activity_at` (`NULL` when the session has no timestamped message). The
/// query's `WHERE`/`ORDER BY` key is the coalesced `recency`, but that is
/// derivable from `last_activity_at`/`created_at` and not returned.
fn page_row_from_row(row: &Row<'_>) -> Result<SessionPageRow> {
    let session = session_from_parts(map_session(row)?)?;
    let last_activity_at: Option<String> = row.get(6)?;
    Ok((session, last_activity_at))
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
    })
}

fn permission_request_from_row(row: &Row<'_>) -> Result<PermissionRequest> {
    Ok(PermissionRequest {
        id: row.get(0)?,
        session_id: SessionId::from(row.get::<_, String>(1)?),
        tool_name: row.get(2)?,
        tool_input_json: row.get(3)?,
        tool_use_id: row.get(4)?,
        status: PermissionStatus::parse(&row.get::<_, String>(5)?)?,
        decision_reason: row.get(6)?,
        created_at: row.get(7)?,
        decided_at: row.get(8)?,
    })
}

/// Map a `launch_option` row, in `LAUNCH_OPTION_COLS` order. Every column maps
/// directly to its domain field (no fallible status/enum parse), so this mirrors
/// [`map_session`] and returns the raw `rusqlite::Result`.
fn launch_option_from_row(row: &Row<'_>) -> rusqlite::Result<LaunchOption> {
    Ok(LaunchOption {
        id: row.get(0)?,
        label: row.get(1)?,
        name: row.get(2)?,
        value: row.get(3)?,
        // SQLite stores the bool as INTEGER 0/1; `rusqlite` maps it back to `bool`.
        default_enabled: row.get(4)?,
        created_at: row.get(5)?,
    })
}

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
    })
}

/// Look up a single session row by id, mapping it into a [`Session`].
fn query_session_by_id(conn: &Connection, id: &SessionId) -> Result<Option<Session>> {
    let parts = conn
        .query_row(
            &format!("SELECT {SESSION_COLS} FROM session WHERE id = ?1"),
            params![id.as_str()],
            map_session,
        )
        .optional()
        .map_err(Error::from)?;
    match parts {
        Some(parts) => Ok(Some(session_from_parts(parts)?)),
        None => Ok(None),
    }
}

const SESSION_COLS: &str = "id, cwd, transcript_path, title, status, created_at";
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
const SEND_COLS: &str =
    "id, session_id, thread_id, semantic_parent_uuid, text, locator_quote, status, matched_uuid, created_at";
const MESSAGE_COLS: &str = "uuid, session_id, thread_id, role, linear_parent_uuid, semantic_parent_uuid, prompt_id, seq, content_text, content_json, created_at, model, git_branch, cwd, response_time_ms";
const LAUNCH_OPTION_COLS: &str = "id, label, name, value, default_enabled, created_at";

/// Ensure the session's `main` thread exists, returning its id.
fn ensure_main_thread(conn: &Connection, id: &SessionId, now: &str) -> Result<ThreadId> {
    let main_id: Option<i64> = conn
        .query_row(
            "SELECT id FROM thread WHERE session_id = ?1 AND title = ?2 ORDER BY id LIMIT 1",
            params![id.as_str(), MAIN_THREAD_TITLE],
            |r| r.get(0),
        )
        .optional()?;

    let main_id = match main_id {
        Some(id) => id,
        None => {
            conn.execute(
                "INSERT INTO thread (session_id, title, parent_thread_id, created_at)
                 VALUES (?1, ?2, NULL, ?3)",
                params![id.as_str(), MAIN_THREAD_TITLE, now],
            )?;
            conn.last_insert_rowid()
        }
    };
    Ok(ThreadId(main_id))
}

#[async_trait]
impl SessionStore for SqliteStore {
    async fn register_session(
        &self,
        new: NewSession,
    ) -> std::result::Result<(Session, ThreadId), delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let now = now_iso8601();

        // Insert the session if absent. When the row already exists as a
        // Delta-launched `spawning` session (inserted eagerly when the id was
        // minted), this first hook contact activates it: the status flips to
        // `active` and the hook-reported transcript path (unknown at mint time)
        // is filled in. An already-active/ended row is left untouched.
        conn.execute(
            "INSERT INTO session (id, cwd, transcript_path, title, status, created_at)
             VALUES (?1, ?2, ?3, NULL, 'active', ?4)
             ON CONFLICT(id) DO UPDATE SET
               cwd = excluded.cwd,
               transcript_path = excluded.transcript_path,
               status = 'active'
             WHERE session.status = 'spawning'",
            params![new.id.as_str(), new.cwd, new.transcript_path, now],
        )
        .map_err(Error::from)?;

        let session =
            query_session_by_id(&conn, &new.id)?.expect("session row exists after upsert");

        let main_id = ensure_main_thread(&conn, &new.id, &now)?;
        Ok((session, main_id))
    }

    async fn insert_spawning_session(
        &self,
        id: &SessionId,
        cwd: &str,
    ) -> std::result::Result<(Session, ThreadId), delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let now = now_iso8601();
        // A plain INSERT: the id is a freshly-minted UUID v7, so a conflict is
        // a programming error worth surfacing, not a case to paper over.
        conn.execute(
            "INSERT INTO session (id, cwd, transcript_path, title, status, created_at)
             VALUES (?1, ?2, NULL, NULL, 'spawning', ?3)",
            params![id.as_str(), cwd, now],
        )
        .map_err(Error::from)?;
        let main_id = ensure_main_thread(&conn, id, &now)?;
        Ok((
            Session {
                id: id.clone(),
                cwd: cwd.to_owned(),
                transcript_path: None,
                title: None,
                status: SessionStatus::Spawning,
                created_at: now,
            },
            main_id,
        ))
    }

    async fn delete_session(
        &self,
        id: &SessionId,
    ) -> std::result::Result<(), delta_usecase::Error> {
        let conn = self.conn.lock().await;
        // Cascades clean every child row (threads, messages, sends, permission
        // requests, the sync cursor).
        conn.execute("DELETE FROM session WHERE id = ?1", params![id.as_str()])
            .map_err(Error::from)?;
        Ok(())
    }

    async fn mark_session_failed(
        &self,
        id: &SessionId,
    ) -> std::result::Result<(), delta_usecase::Error> {
        let conn = self.conn.lock().await;
        // Only a still-spawning session can fail to launch; an already-active
        // session must never be flipped to `failed` by a stale reap.
        conn.execute(
            "UPDATE session SET status = 'failed' WHERE id = ?1 AND status = 'spawning'",
            params![id.as_str()],
        )
        .map_err(Error::from)?;
        Ok(())
    }

    async fn list_sessions_page(
        &self,
        cursor: Option<SessionPageCursor>,
        limit: u32,
    ) -> std::result::Result<Vec<SessionPageRow>, delta_usecase::Error> {
        let conn = self.conn.lock().await;

        // A `spawning` session that has ingested nothing is excluded: its
        // launch has not produced a single hook yet, so listing it would
        // surface a row the browser cannot open, and the optimistic new-session
        // pending chip would mis-bind to it before the spawn either activates
        // (it then lists as `active`, exactly when it used to appear) or fails
        // (the row is reaped). The message guard keeps the predicate honest if
        // a spawning session ever held data.
        //
        // `recency` is the row's last activity, falling back to its own
        // `created_at` when message-less — read straight from the denormalized
        // `last_activity_at` column, NOT recomputed per row. The ordering is
        // `recency` DESC, then `created_at` DESC, then `id` DESC, satisfied by
        // `ix_session_last_activity (last_activity_at, created_at, id)` so LIMIT
        // bounds the scan instead of sorting every session. The final
        // tiebreaker is descending because Delta-minted session ids are
        // time-ordered UUID v7: when two sessions tie on both timestamps (they
        // have second resolution, so a burst of activity ties easily), the
        // *newest* session must still sort first — most-recently-active first
        // all the way down. The cursor predicate is the expanded OR form
        // (equivalent to a row-value tuple comparison) so each key's role stays
        // explicit. When there is no cursor, `:cursor_null = 1` short-circuits
        // the predicate to select from the top. ISO-8601 UTC timestamps compare
        // correctly as text, so no datetime casting is needed. The message-less
        // `spawning` exclusion uses `last_activity_at IS NULL` as a cheap
        // necessary condition (a spawning session that ingested nothing has no
        // activity), then confirms with the message guard.
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {SESSION_COLS}, \
                 last_activity_at, \
                 COALESCE(last_activity_at, created_at) AS recency \
                 FROM session \
                 WHERE NOT (status = 'spawning' AND last_activity_at IS NULL \
                            AND NOT EXISTS (SELECT 1 FROM message m WHERE m.session_id = session.id)) \
                   AND (:cursor_null = 1 \
                    OR recency < :r \
                    OR (recency = :r AND (created_at < :c OR (created_at = :c AND id < :i)))) \
                 ORDER BY recency DESC, created_at DESC, id DESC \
                 LIMIT :limit"
            ))
            .map_err(Error::from)?;

        // Bind cursor components even when absent: the `:cursor_null = 1` guard
        // makes the comparisons inert, but every named parameter must still be
        // supplied. Empty strings are harmless placeholders in that case.
        let cursor_null = if cursor.is_some() { 0 } else { 1 };
        let recency = cursor.as_ref().map(|c| c.recency.as_str()).unwrap_or("");
        let created_at = cursor.as_ref().map(|c| c.created_at.as_str()).unwrap_or("");
        let id = cursor.as_ref().map(|c| c.id.as_str()).unwrap_or("");

        let rows = stmt
            .query_map(
                named_params! {
                    ":cursor_null": cursor_null,
                    ":r": recency,
                    ":c": created_at,
                    ":i": id,
                    ":limit": limit,
                },
                |row| Ok(page_row_from_row(row)),
            )
            .map_err(Error::from)?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(Error::from)??);
        }
        Ok(out)
    }

    async fn session(
        &self,
        id: &SessionId,
    ) -> std::result::Result<Option<Session>, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        Ok(query_session_by_id(&conn, id)?)
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

    async fn recent_workdirs(
        &self,
        limit: u32,
    ) -> std::result::Result<Vec<RecentWorkdir>, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        // One row per distinct `cwd`, ordered by the most recent activity of any
        // session that ran in it. Per-session recency is
        // `COALESCE(last_activity_at, created_at)` — the same denormalized key
        // the session list uses, read straight from the column rather than
        // recomputed with a correlated `MAX(message.created_at)` subquery — and
        // a cwd's recency is the max of that across its sessions. ISO-8601 UTC
        // text compares correctly as time, so no datetime casting is needed.
        let mut stmt = conn
            .prepare(
                "SELECT s.cwd, \
                        MAX(COALESCE(s.last_activity_at, s.created_at)) AS recency \
                 FROM session s \
                 GROUP BY s.cwd \
                 ORDER BY recency DESC, s.cwd ASC \
                 LIMIT ?1",
            )
            .map_err(Error::from)?;
        let rows = stmt
            .query_map(params![limit], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })
            .map_err(Error::from)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(Error::from)?);
        }
        Ok(out)
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

    async fn enqueue_send(
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
        })
    }

    async fn enqueue_queued_send(
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
        })
    }

    async fn send(&self, id: i64) -> std::result::Result<Option<Send>, delta_usecase::Error> {
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

    async fn next_queued_send(
        &self,
        session_id: &SessionId,
    ) -> std::result::Result<Option<Send>, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let row = conn
            .query_row(
                &format!(
                    "SELECT {SEND_COLS} FROM send
                     WHERE session_id = ?1 AND status = 'queued'
                     ORDER BY id LIMIT 1"
                ),
                params![session_id.as_str()],
                |r| Ok(send_from_row(r)),
            )
            .optional()
            .map_err(Error::from)?;
        row.transpose().map_err(Into::into)
    }

    async fn open_sends(
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

    async fn promote_queued_send(&self, id: i64) -> std::result::Result<(), delta_usecase::Error> {
        let conn = self.conn.lock().await;
        conn.execute(
            "UPDATE send SET status = 'dispatched' WHERE id = ?1 AND status = 'queued'",
            params![id],
        )
        .map_err(Error::from)?;
        Ok(())
    }

    async fn requeue_send(&self, id: i64) -> std::result::Result<(), delta_usecase::Error> {
        let conn = self.conn.lock().await;
        conn.execute(
            "UPDATE send SET status = 'queued' WHERE id = ?1 AND status = 'dispatched'",
            params![id],
        )
        .map_err(Error::from)?;
        Ok(())
    }

    async fn head_dispatched_send(
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

    async fn mark_send_matched(
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

    async fn latest_user_thread(
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

    async fn cancel_send(&self, id: i64) -> std::result::Result<(), delta_usecase::Error> {
        let conn = self.conn.lock().await;
        conn.execute(
            "UPDATE send SET status = 'cancelled' WHERE id = ?1",
            params![id],
        )
        .map_err(Error::from)?;
        Ok(())
    }

    async fn cancel_queued_send(&self, id: i64) -> std::result::Result<bool, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let affected = conn
            .execute(
                "UPDATE send SET status = 'cancelled' WHERE id = ?1 AND status = 'queued'",
                params![id],
            )
            .map_err(Error::from)?;
        Ok(affected > 0)
    }

    async fn upsert_messages(
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
                     (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
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
                       response_time_ms = excluded.response_time_ms"
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

    async fn last_activity_at(
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

    async fn transcript_lines_read(
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

    async fn set_transcript_lines_read(
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
        tool_use_id: Option<&str>,
    ) -> std::result::Result<PermissionRequest, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let now = now_iso8601();
        conn.execute(
            "INSERT INTO permission_request
             (session_id, tool_name, tool_input_json, tool_use_id, status, decision_reason, created_at, decided_at)
             VALUES (?1, ?2, ?3, ?4, 'pending', NULL, ?5, NULL)",
            params![
                session_id.as_str(),
                tool_name,
                tool_input_json,
                tool_use_id,
                now
            ],
        )
        .map_err(Error::from)?;
        let id = conn.last_insert_rowid();
        Ok(PermissionRequest {
            id,
            session_id: session_id.clone(),
            tool_name: tool_name.to_owned(),
            tool_input_json: tool_input_json.to_owned(),
            tool_use_id: tool_use_id.map(str::to_owned),
            status: PermissionStatus::Pending,
            decision_reason: None,
            created_at: now,
            decided_at: None,
        })
    }

    async fn decide_permission_request(
        &self,
        request_id: i64,
        allowed: bool,
    ) -> std::result::Result<Option<PermissionRequest>, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let now = now_iso8601();
        let status = if allowed {
            PermissionStatus::Allowed
        } else {
            PermissionStatus::Denied
        };
        // Decide only a still-`pending` row, so a late decision can never flip
        // one already settled (by an earlier decision or a tool_result).
        let row = conn
            .query_row(
                "UPDATE permission_request
                 SET status = ?1, decided_at = ?2
                 WHERE id = ?3 AND status = 'pending'
                 RETURNING id, session_id, tool_name, tool_input_json, tool_use_id,
                           status, decision_reason, created_at, decided_at",
                params![status.as_str(), now, request_id],
                |r| Ok(permission_request_from_row(r)),
            )
            .optional()
            .map_err(Error::from)?;
        row.transpose().map_err(Into::into)
    }

    async fn resolve_permission_by_tool_use_id(
        &self,
        session_id: &SessionId,
        tool_use_id: &str,
        allowed: bool,
    ) -> std::result::Result<Vec<i64>, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let now = now_iso8601();
        let status = if allowed {
            PermissionStatus::Allowed
        } else {
            PermissionStatus::Denied
        };
        // Resolve only still-`pending` rows, so a re-ingested tool_result
        // cannot flip an already-decided row. Two kinds of row settle here:
        // the `PreToolUse`-recorded row whose `tool_use_id` matches, and any
        // pending dialog row owned by the `PermissionRequest` hook
        // (`tool_use_id IS NULL`) — the dialog blocks the session, so the next
        // tool_result to arrive is the one it gated. The returned ids are the
        // rows that transitioned, one `PermissionResolved` each.
        let mut stmt = conn
            .prepare(
                "UPDATE permission_request
                 SET status = ?1, decided_at = ?2
                 WHERE session_id = ?3 AND status = 'pending'
                   AND (tool_use_id = ?4 OR tool_use_id IS NULL)
                 RETURNING id",
            )
            .map_err(Error::from)?;
        let ids = stmt
            .query_map(
                params![status.as_str(), now, session_id.as_str(), tool_use_id],
                |row| row.get::<_, i64>(0),
            )
            .map_err(Error::from)?
            .collect::<std::result::Result<Vec<i64>, _>>()
            .map_err(Error::from)?;
        Ok(ids)
    }

    async fn record_subagent_launch(
        &self,
        session_id: &SessionId,
        tool_use_id: &str,
        thread_id: ThreadId,
    ) -> std::result::Result<(), delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let now = now_iso8601();
        // Idempotent: re-seeing the same launch (e.g. a re-ingested batch)
        // refreshes the row rather than erroring on the primary key.
        conn.execute(
            "INSERT INTO subagent_launch (session_id, tool_use_id, thread_id, created_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(session_id, tool_use_id) DO UPDATE SET
               thread_id = excluded.thread_id",
            params![session_id.as_str(), tool_use_id, thread_id.value(), now],
        )
        .map_err(Error::from)?;
        Ok(())
    }

    async fn clear_subagent_launch(
        &self,
        session_id: &SessionId,
        tool_use_id: &str,
    ) -> std::result::Result<(), delta_usecase::Error> {
        let conn = self.conn.lock().await;
        conn.execute(
            "DELETE FROM subagent_launch WHERE session_id = ?1 AND tool_use_id = ?2",
            params![session_id.as_str(), tool_use_id],
        )
        .map_err(Error::from)?;
        Ok(())
    }

    async fn outstanding_subagent_launches(
        &self,
        session_id: &SessionId,
    ) -> std::result::Result<BTreeMap<String, ThreadId>, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let mut stmt = conn
            .prepare(
                "SELECT tool_use_id, thread_id FROM subagent_launch WHERE session_id = ?1",
            )
            .map_err(Error::from)?;
        let rows = stmt
            .query_map(params![session_id.as_str()], |row| {
                Ok((row.get::<_, String>(0)?, ThreadId(row.get::<_, i64>(1)?)))
            })
            .map_err(Error::from)?;
        let mut out = BTreeMap::new();
        for row in rows {
            let (tool_use_id, thread_id) = row.map_err(Error::from)?;
            out.insert(tool_use_id, thread_id);
        }
        Ok(out)
    }

    async fn list_launch_options(
        &self,
    ) -> std::result::Result<Vec<LaunchOption>, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        // Newest first: the most recently registered option is the one a user is
        // most likely to be looking for in the settings list.
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {LAUNCH_OPTION_COLS} FROM launch_option ORDER BY id DESC"
            ))
            .map_err(Error::from)?;
        let rows = stmt
            .query_map([], launch_option_from_row)
            .map_err(Error::from)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(Error::from)?);
        }
        Ok(out)
    }

    async fn create_launch_option(
        &self,
        label: Option<&str>,
        name: &str,
        value: Option<&str>,
        default_enabled: bool,
    ) -> std::result::Result<LaunchOption, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let now = now_iso8601();
        conn.execute(
            "INSERT INTO launch_option (label, name, value, default_enabled, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![label, name, value, default_enabled, now],
        )
        .map_err(Error::from)?;
        let id = conn.last_insert_rowid();
        Ok(LaunchOption {
            id,
            label: label.map(str::to_owned),
            name: name.to_owned(),
            value: value.map(str::to_owned),
            default_enabled,
            created_at: now,
        })
    }

    async fn set_launch_option_default_enabled(
        &self,
        id: i64,
        default_enabled: bool,
    ) -> std::result::Result<Option<LaunchOption>, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let affected = conn
            .execute(
                "UPDATE launch_option SET default_enabled = ?2 WHERE id = ?1",
                params![id, default_enabled],
            )
            .map_err(Error::from)?;
        if affected == 0 {
            return Ok(None);
        }
        let option = conn
            .query_row(
                &format!("SELECT {LAUNCH_OPTION_COLS} FROM launch_option WHERE id = ?1"),
                params![id],
                launch_option_from_row,
            )
            .optional()
            .map_err(Error::from)?;
        Ok(option)
    }

    async fn delete_launch_option(
        &self,
        id: i64,
    ) -> std::result::Result<(), delta_usecase::Error> {
        let conn = self.conn.lock().await;
        conn.execute("DELETE FROM launch_option WHERE id = ?1", params![id])
            .map_err(Error::from)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
