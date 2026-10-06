//! Session rows: registration, spawn lifecycle, and the recency-ordered
//! session-list page. The history aggregations over these rows live in
//! `session_history`.

use rusqlite::{named_params, params, Connection, OptionalExtension, Row};

use delta_model::{AgentProvider, Session, SessionId, SessionStatus, ThreadId};
use delta_usecase::{NewSession, SessionPageCursor, SessionPageRow, SpawningSession};

use crate::error::{Error, Result};
use crate::time::now_iso8601;

use super::{ensure_main_thread, SqliteStore};

/// The raw `session` columns of one row, in `SESSION_COLS` order, before the
/// status string is parsed into a domain [`Session`].
struct SessionParts {
    id: SessionId,
    cwd: String,
    transcript_path: Option<String>,
    title: Option<String>,
    status: String,
    created_at: String,
    branch_at_launch: Option<String>,
    repo_root: Option<String>,
    requested_workdir: Option<String>,
    repository_display_name: Option<String>,
    provider: String,
    provider_session_id: Option<String>,
    provider_thread_id: Option<String>,
    pull_request_number: Option<i64>,
    failure_reason: Option<String>,
}

fn map_session(row: &Row<'_>) -> rusqlite::Result<SessionParts> {
    Ok(SessionParts {
        id: SessionId::from(row.get::<_, String>(0)?),
        cwd: row.get(1)?,
        transcript_path: row.get(2)?,
        title: row.get(3)?,
        status: row.get(4)?,
        created_at: row.get(5)?,
        branch_at_launch: row.get(6)?,
        repo_root: row.get(7)?,
        requested_workdir: row.get(8)?,
        repository_display_name: row.get(9)?,
        provider: row.get(10)?,
        provider_session_id: row.get(11)?,
        provider_thread_id: row.get(12)?,
        pull_request_number: row.get(13)?,
        failure_reason: row.get(14)?,
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
        branch_at_launch: parts.branch_at_launch,
        repo_root: parts.repo_root,
        requested_workdir: parts.requested_workdir,
        repository_display_name: parts.repository_display_name,
        provider: AgentProvider::parse(&parts.provider)?,
        provider_session_id: parts.provider_session_id,
        provider_thread_id: parts.provider_thread_id,
        pull_request_number: parts.pull_request_number,
        failure_reason: parts.failure_reason,
    })
}

/// Map a session-list page row: the session columns followed by the stored
/// `last_activity_at` (`NULL` when the session has no timestamped message). The
/// query's `WHERE`/`ORDER BY` key is the coalesced `recency`, but that is
/// derivable from `last_activity_at`/`created_at` and not returned.
fn page_row_from_row(row: &Row<'_>) -> Result<SessionPageRow> {
    let session = session_from_parts(map_session(row)?)?;
    // `last_activity_at` follows the `SESSION_COLS` block, so its positional
    // index is the column count of `SESSION_COLS` (15) — the first column after
    // the session fields.
    let last_activity_at: Option<String> = row.get(15)?;
    Ok((session, last_activity_at))
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

const SESSION_COLS: &str = "id, cwd, transcript_path, title, status, created_at, \
     branch_at_launch, repo_root, requested_workdir, repository_display_name, \
     provider, provider_session_id, provider_thread_id, pull_request_number, \
     failure_reason";

impl SqliteStore {
    pub(super) async fn register_session(
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
        //
        // `branch_at_launch`, `repo_root`, `repository_display_name` and
        // `pull_request_number` are NOT touched on the activate path: the eager
        // spawn has already recorded the launch-time snapshot via
        // `insert_spawning_session`, and an externally-started `claude` (the
        // fresh-insert path here) has no Delta-known launch context at all, so
        // all four stay NULL for it.
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

    pub(super) async fn insert_spawning_session(
        &self,
        spawning: SpawningSession<'_>,
    ) -> std::result::Result<(Session, ThreadId), delta_usecase::Error> {
        let SpawningSession {
            id,
            cwd,
            branch_at_launch,
            repo_root,
            requested_workdir,
            repository_display_name,
            provider,
            pull_request_number,
        } = spawning;
        let conn = self.conn.lock().await;
        let now = now_iso8601();
        // A plain INSERT: the id is a freshly-minted UUID v7, so a conflict is
        // a programming error worth surfacing, not a case to paper over. The
        // spawn-time git snapshot (`branch_at_launch`, `repo_root`,
        // `repository_display_name`) and the user-selected `requested_workdir`
        // are written once here and never updated later — see the doc on
        // `Session`. `pull_request_number` is the same kind of snapshot: the PR
        // the session was opened from, NULL for every other origin. `provider`
        // records the backend; the provider-minted conversation ids are unknown
        // until launch returns, so they stay NULL here and are filled later via
        // `set_provider_ids`.
        conn.execute(
            "INSERT INTO session
             (id, cwd, transcript_path, title, status, created_at,
              branch_at_launch, repo_root, requested_workdir, repository_display_name,
              provider, pull_request_number)
             VALUES (?1, ?2, NULL, NULL, 'spawning', ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                id.as_str(),
                cwd,
                now,
                branch_at_launch,
                repo_root,
                requested_workdir,
                repository_display_name,
                provider.as_str(),
                pull_request_number,
            ],
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
                branch_at_launch: branch_at_launch.map(str::to_owned),
                repo_root: repo_root.map(str::to_owned),
                requested_workdir: requested_workdir.map(str::to_owned),
                repository_display_name: repository_display_name.map(str::to_owned),
                provider,
                provider_session_id: None,
                provider_thread_id: None,
                pull_request_number,
                failure_reason: None,
            },
            main_id,
        ))
    }

    pub(super) async fn set_provider_ids(
        &self,
        id: &SessionId,
        provider_session_id: Option<&str>,
        provider_thread_id: Option<&str>,
    ) -> std::result::Result<(), delta_usecase::Error> {
        let conn = self.conn.lock().await;
        // Record the provider-minted ids and, if the row is still `spawning`,
        // activate it (spawning → active). This is the structured-provider
        // analogue of `register_session`'s first-hook activation: a terminal-less
        // provider (Codex) has no hook to flip the status, so the launch-return
        // that yields these ids is what confirms the session exists. An
        // already-active/ended row keeps its status (the CASE else branch).
        conn.execute(
            "UPDATE session
             SET provider_session_id = ?2,
                 provider_thread_id = ?3,
                 status = CASE WHEN status = 'spawning' THEN 'active' ELSE status END
             WHERE id = ?1",
            params![id.as_str(), provider_session_id, provider_thread_id],
        )
        .map_err(Error::from)?;
        Ok(())
    }

    pub(super) async fn delete_session(
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

    pub(super) async fn list_prunable_sessions(
        &self,
        cutoff: &str,
        statuses: &[SessionStatus],
    ) -> std::result::Result<Vec<SessionId>, delta_usecase::Error> {
        // No status matches nothing; short-circuit rather than building an
        // `IN ()`, which SQLite rejects.
        if statuses.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.conn.lock().await;
        // The recency key is the session list's, so "old" means what the
        // navigator's ordering says it means; ISO-8601 UTC timestamps compare
        // correctly as text. `?1` is the cut-off, `?2..` the statuses.
        let placeholders = (2..=statuses.len() + 1)
            .map(|i| format!("?{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        let mut stmt = conn
            .prepare(&format!(
                "SELECT id FROM session \
                 WHERE COALESCE(last_activity_at, created_at) <= ?1 \
                   AND status IN ({placeholders}) \
                 ORDER BY COALESCE(last_activity_at, created_at) ASC, created_at ASC, id ASC"
            ))
            .map_err(Error::from)?;
        let params = std::iter::once(cutoff).chain(statuses.iter().map(|status| status.as_str()));
        let rows = stmt
            .query_map(rusqlite::params_from_iter(params), |row| {
                row.get::<_, String>(0)
            })
            .map_err(Error::from)?;
        let mut ids = Vec::new();
        for row in rows {
            ids.push(SessionId::from(row.map_err(Error::from)?));
        }
        Ok(ids)
    }

    pub(super) async fn mark_session_failed(
        &self,
        id: &SessionId,
        reason: Option<&str>,
    ) -> std::result::Result<(), delta_usecase::Error> {
        let conn = self.conn.lock().await;
        // Only a still-spawning session can fail to launch; an already-active
        // session must never be flipped to `failed` by a stale reap. The reason
        // is written in the same statement — including the NULL a
        // watchdog-shaped ending reports — so the column always describes the
        // ending that set the status, never one from some earlier attempt.
        conn.execute(
            "UPDATE session SET status = 'failed', failure_reason = ?2 \
             WHERE id = ?1 AND status = 'spawning'",
            params![id.as_str(), reason],
        )
        .map_err(Error::from)?;
        Ok(())
    }

    pub(super) async fn relocate_transcript(
        &self,
        id: &SessionId,
        transcript_path: &str,
        cwd: Option<&str>,
    ) -> std::result::Result<(), delta_usecase::Error> {
        let conn = self.conn.lock().await;
        // Unlike the `register_session` upsert, which fills the path in only
        // while the row is `spawning`, this applies in every status: Claude
        // Code moves a live session's transcript when it enters a worktree.
        conn.execute(
            "UPDATE session SET transcript_path = ?2, cwd = COALESCE(?3, cwd) WHERE id = ?1",
            params![id.as_str(), transcript_path, cwd],
        )
        .map_err(Error::from)?;
        Ok(())
    }

    pub(super) async fn list_sessions_page(
        &self,
        cursor: Option<SessionPageCursor>,
        limit: u32,
    ) -> std::result::Result<Vec<SessionPageRow>, delta_usecase::Error> {
        let conn = self.conn.lock().await;

        // Every session row is listed, including a still-`spawning` one that
        // has ingested nothing: the browser shows a session from the moment
        // its first send is accepted, as a starting session, rather than
        // parking the user on the new-session screen until the launch's first
        // hook arrives. A spawn that never binds is marked `failed` and stays
        // listed — the user opens it to read why it did not start, retries it,
        // or removes it.
        //
        // `recency` is the row's last activity, falling back to its own
        // `created_at` when message-less — read straight from the denormalized
        // `last_activity_at` column, NOT recomputed per row. The ordering is
        // `recency` DESC, then `created_at` DESC, then `id` DESC, satisfied by
        // the expression index `ix_session_recency
        // (COALESCE(last_activity_at, created_at) DESC, created_at DESC,
        // id DESC)` so LIMIT bounds the scan instead of sorting every session
        // (`list_sessions_page_uses_the_recency_index` asserts the plan). The final
        // tiebreaker is descending because Delta-minted session ids are
        // time-ordered UUID v7: when two sessions tie on both timestamps (they
        // have second resolution, so a burst of activity ties easily), the
        // *newest* session must still sort first — most-recently-active first
        // all the way down. The cursor predicate is the expanded OR form
        // (equivalent to a row-value tuple comparison) so each key's role stays
        // explicit. When there is no cursor, `:cursor_null = 1` short-circuits
        // the predicate to select from the top. ISO-8601 UTC timestamps compare
        // correctly as text, so no datetime casting is needed.
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {SESSION_COLS}, \
                 last_activity_at, \
                 COALESCE(last_activity_at, created_at) AS recency \
                 FROM session \
                 WHERE (:cursor_null = 1 \
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

    pub(super) async fn list_sessions_by_ids(
        &self,
        ids: &[SessionId],
    ) -> std::result::Result<Vec<SessionPageRow>, delta_usecase::Error> {
        // An empty id list matches nothing by definition; short-circuit rather
        // than building an `IN ()` (which SQLite rejects) or taking the lock.
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.conn.lock().await;

        // Same row shape and same ordering key as `list_sessions_page`, so the
        // open-first head of the session list is internally ordered exactly like
        // the closed stream that follows it. Selection is by id rather than by
        // cursor: the caller already holds the (live-pane-bounded) id set. An id
        // with no row simply matches nothing — an accepted spawn reaped between
        // the liveness snapshot and this query drops out silently.
        let placeholders = (1..=ids.len())
            .map(|i| format!("?{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {SESSION_COLS}, \
                 last_activity_at, \
                 COALESCE(last_activity_at, created_at) AS recency \
                 FROM session \
                 WHERE id IN ({placeholders}) \
                 ORDER BY recency DESC, created_at DESC, id DESC"
            ))
            .map_err(Error::from)?;

        let rows = stmt
            .query_map(
                rusqlite::params_from_iter(ids.iter().map(|id| id.as_str())),
                |row| Ok(page_row_from_row(row)),
            )
            .map_err(Error::from)?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(Error::from)??);
        }
        Ok(out)
    }

    pub(super) async fn session(
        &self,
        id: &SessionId,
    ) -> std::result::Result<Option<Session>, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        Ok(query_session_by_id(&conn, id)?)
    }
}
