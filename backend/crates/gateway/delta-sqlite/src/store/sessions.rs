//! Session rows: registration, spawn lifecycle, the recency-ordered
//! session-list page, and the workdir/repository queries derived from them.

use rusqlite::{named_params, params, Connection, OptionalExtension, Row};

use delta_model::{AgentProvider, Session, SessionId, SessionStatus, ThreadId};
use delta_usecase::{
    NewSession, RecentWorkdir, RepositoryCloneRow, SessionPageCursor, SessionPageRow,
};

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
    })
}

/// Map a session-list page row: the session columns followed by the stored
/// `last_activity_at` (`NULL` when the session has no timestamped message). The
/// query's `WHERE`/`ORDER BY` key is the coalesced `recency`, but that is
/// derivable from `last_activity_at`/`created_at` and not returned.
fn page_row_from_row(row: &Row<'_>) -> Result<SessionPageRow> {
    let session = session_from_parts(map_session(row)?)?;
    // `last_activity_at` follows the `SESSION_COLS` block, so its positional
    // index is the column count of `SESSION_COLS` (13) — the first column after
    // the session fields.
    let last_activity_at: Option<String> = row.get(13)?;
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
     provider, provider_session_id, provider_thread_id";

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
        // `branch_at_launch`, `repo_root`, and `repository_display_name` are
        // NOT touched on the activate path: the eager spawn has already
        // recorded the launch-time snapshot via `insert_spawning_session`, and
        // an externally-started `claude` (the fresh-insert path here) has no
        // Delta-known launch git context, so all three stay NULL for it.
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

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn insert_spawning_session(
        &self,
        id: &SessionId,
        cwd: &str,
        branch_at_launch: Option<&str>,
        repo_root: Option<&str>,
        requested_workdir: Option<&str>,
        repository_display_name: Option<&str>,
        provider: AgentProvider,
    ) -> std::result::Result<(Session, ThreadId), delta_usecase::Error> {
        let conn = self.conn.lock().await;
        let now = now_iso8601();
        // A plain INSERT: the id is a freshly-minted UUID v7, so a conflict is
        // a programming error worth surfacing, not a case to paper over. The
        // spawn-time git snapshot (`branch_at_launch`, `repo_root`,
        // `repository_display_name`) and the user-selected `requested_workdir`
        // are written once here and never updated later — see the doc on
        // `Session`. `provider` records the backend; the provider-minted
        // conversation ids are unknown until launch returns, so they stay NULL
        // here and are filled later via `set_provider_ids`.
        conn.execute(
            "INSERT INTO session
             (id, cwd, transcript_path, title, status, created_at,
              branch_at_launch, repo_root, requested_workdir, repository_display_name,
              provider)
             VALUES (?1, ?2, NULL, NULL, 'spawning', ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                id.as_str(),
                cwd,
                now,
                branch_at_launch,
                repo_root,
                requested_workdir,
                repository_display_name,
                provider.as_str(),
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

    pub(super) async fn mark_session_failed(
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
        // hook arrives. A spawn that never binds is reaped, so its row leaves
        // the list again (the client hears `spawn_failed`).
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

    pub(super) async fn session(
        &self,
        id: &SessionId,
    ) -> std::result::Result<Option<Session>, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        Ok(query_session_by_id(&conn, id)?)
    }

    pub(super) async fn recent_workdirs(
        &self,
        limit: u32,
    ) -> std::result::Result<Vec<RecentWorkdir>, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        // One row per distinct workdir, ordered by the most recent activity of
        // any session that ran in it. The grouping key is
        // `COALESCE(requested_workdir, cwd)`: a worktree-on spawn stores the
        // user-selected dir in `requested_workdir` and the auto-generated
        // worktree path in `cwd`, so coalescing pulls the user-selected dir to
        // the surface and the worktree path drops out of Recent. Sessions that
        // predate `requested_workdir` (the column is additive and NULL for
        // them) fall back to `cwd`, so legacy history stays visible.
        //
        // Per-session recency is `COALESCE(last_activity_at, created_at)` — the
        // same denormalized key the session list uses, read straight from the
        // column rather than recomputed with a correlated
        // `MAX(message.created_at)` subquery — and a workdir's recency is the
        // max of that across its sessions. ISO-8601 UTC text compares correctly
        // as time, so no datetime casting is needed.
        let mut stmt = conn
            .prepare(
                "SELECT COALESCE(s.requested_workdir, s.cwd) AS workdir, \
                        MAX(COALESCE(s.last_activity_at, s.created_at)) AS recency \
                 FROM session s \
                 GROUP BY workdir \
                 ORDER BY recency DESC, workdir ASC \
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

    pub(super) async fn cwd_exists(
        &self,
        path: &str,
    ) -> std::result::Result<bool, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        // Match `path` verbatim against any of the three columns the browser
        // ever sees a cwd from: `session.cwd`, `session.requested_workdir`
        // (both surfaced on the session card), and `message.cwd` (the
        // per-turn cwd on the message meta line). The path comparison is
        // byte-for-byte — the browser echoes back the same string the server
        // sent, so no normalisation is needed here.
        //
        // `SELECT EXISTS` short-circuits on the first match and both scans
        // use existing indexes on their session_id foreign keys, so the
        // query stays cheap even on a large history.
        let hit: i64 = conn
            .query_row(
                "SELECT EXISTS ( \
                     SELECT 1 FROM session \
                       WHERE cwd = ?1 OR requested_workdir = ?1 \
                     UNION ALL \
                     SELECT 1 FROM message WHERE cwd = ?1 \
                 )",
                params![path],
                |row| row.get(0),
            )
            .map_err(Error::from)?;
        Ok(hit != 0)
    }

    pub(super) async fn repository_clone_rows(
        &self,
        worktree_base: &str,
        active_repo_limit: i64,
        user_clone_limit: i64,
        generated_clone_limit: i64,
    ) -> std::result::Result<Vec<RepositoryCloneRow>, delta_usecase::Error> {
        let conn = self.conn.lock().await;
        // One row per `(repo_root, clone_path)` pair, drawn from sessions with
        // a non-null repo_root, then bounded by per-repo and per-kind caps so
        // the Repository tab cannot grow without limit as new worktree-on
        // spawns are recorded.
        //
        // The CTE pipeline runs in four steps:
        //
        // 1. `ranked` — coalesce `requested_workdir` with `cwd` so
        //    worktree-managed cwds do not leak in, classify each row as
        //    `generated` (lies under `worktree_base + '/'`) or `user`
        //    otherwise, and pick the most-recent session per
        //    `(repo_root, clone_path)` pair via `ROW_NUMBER()`. SQLite has
        //    supported window functions since 3.25 (2018), well below the
        //    minimum the rest of the store assumes.
        // 2. `latest` — keep only `rn = 1` from `ranked`: one row per pair,
        //    carrying its `branch_at_launch` and the max recency at that pair.
        // 3. `active_roots` — take the top `?2` `repo_root`s by their max
        //    recency across `latest`. Older repos drop wholesale; they are
        //    unlikely to be a useful start point for a new session.
        // 4. `windowed` — within each retained `repo_root`, rank by `kind`
        //    (user / generated) and keep at most `?3` user paths and `?4`
        //    generated paths. Separate caps keep a burst of disposable
        //    worktrees from squeezing out user-meaningful clones.
        //
        // ISO-8601 UTC text compares correctly as time, so no datetime
        // casting is needed for the recency key. The `LIKE ?1 || '/%'`
        // classifier rejects a clone path that *equals* `worktree_base` so
        // a stray top-level entry is not misclassified as generated.
        let mut stmt = conn
            .prepare(
                "WITH ranked AS (
                  SELECT s.repo_root,
                         COALESCE(s.requested_workdir, s.cwd) AS clone_path,
                         s.branch_at_launch,
                         COALESCE(s.last_activity_at, s.created_at) AS recency,
                         CASE WHEN COALESCE(s.requested_workdir, s.cwd) LIKE ?1 || '/%' THEN 'generated' ELSE 'user' END AS kind,
                         ROW_NUMBER() OVER (
                           PARTITION BY s.repo_root, COALESCE(s.requested_workdir, s.cwd)
                           ORDER BY COALESCE(s.last_activity_at, s.created_at) DESC, s.id DESC
                         ) AS rn
                  FROM session s
                  WHERE s.repo_root IS NOT NULL
                ),
                latest AS (SELECT * FROM ranked WHERE rn = 1),
                active_roots AS (
                  SELECT repo_root
                  FROM latest
                  GROUP BY repo_root
                  ORDER BY MAX(recency) DESC, repo_root ASC
                  LIMIT ?2
                ),
                windowed AS (
                  SELECT l.*,
                    ROW_NUMBER() OVER (
                      PARTITION BY l.repo_root, l.kind
                      ORDER BY l.recency DESC, l.clone_path ASC
                    ) AS rn_kind
                  FROM latest l
                  JOIN active_roots a ON l.repo_root = a.repo_root
                )
                SELECT repo_root, clone_path, recency AS last_opened_at, branch_at_launch
                FROM windowed
                WHERE (kind = 'user'      AND rn_kind <= ?3)
                   OR (kind = 'generated' AND rn_kind <= ?4)
                ORDER BY recency DESC, repo_root ASC, clone_path ASC",
            )
            .map_err(Error::from)?;
        let rows = stmt
            .query_map(
                params![
                    worktree_base,
                    active_repo_limit,
                    user_clone_limit,
                    generated_clone_limit,
                ],
                |row| {
                    Ok(RepositoryCloneRow {
                        repo_root: row.get::<_, String>(0)?,
                        clone_path: row.get::<_, String>(1)?,
                        last_opened_at: row.get::<_, Option<String>>(2)?,
                        last_branch: row.get::<_, Option<String>>(3)?,
                    })
                },
            )
            .map_err(Error::from)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(Error::from)?);
        }
        Ok(out)
    }
}
