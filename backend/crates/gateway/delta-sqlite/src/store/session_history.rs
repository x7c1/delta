//! History aggregations over the session rows: the recent-workdir list, the
//! "has this cwd ever been seen" check, and the repository clone rows the
//! Repository tab lists.

use rusqlite::params;

use delta_usecase::{RecentWorkdir, RepositoryCloneRow};

use crate::error::Error;

use super::SqliteStore;

impl SqliteStore {
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
