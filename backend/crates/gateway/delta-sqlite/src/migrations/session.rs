//! The `session` table: one row per conversation Delta knows about, plus the
//! index the session list's recency ordering is served by.
//!
//! **Status lifecycle.** A Delta-launched session is INSERTed as `'spawning'`
//! when the id is minted (before `claude` is up), flips to `'active'` when the
//! first hook binds the spawn, and becomes `'failed'` if the spawn never binds
//! before its deadline (a failed session with zero ingested messages is deleted
//! at reap time instead). `transcript_path` is NULL while `'spawning'`: the path
//! is owned by Claude Code and only learned from the first hook.
//!
//! **`last_activity_at` is denormalized on purpose.** It is a copy of the
//! session's most recent message timestamp (`MAX(message.created_at)`),
//! maintained on every message upsert, and NULL while the session has no
//! timestamped message. The session-list queries order by it directly so the
//! ordering is index-backed (`ix_session_recency`) and a LIMIT truly bounds the
//! work, instead of recomputing recency for every session with a correlated
//! subquery and sorting the whole table. The navigator's recency key is
//! `COALESCE(last_activity_at, created_at)`, so a message-less session still
//! sorts on its own `created_at`.
//!
//! Recency is the only key SQL sorts on. The list the browser sees is
//! open-first — live sessions ahead of closed ones, each group by recency — but
//! liveness is process-runtime state with no column here, so that grouping is
//! layered on in the usecase and deliberately kept out of the `ORDER BY` this
//! index serves.
//!
//! **`ix_session_recency` is an expression index**, on
//! `COALESCE(last_activity_at, created_at)` — the navigator's recency key —
//! so the page query's `ORDER BY COALESCE(last_activity_at, created_at) DESC,
//! created_at DESC, id DESC` is satisfied by walking the index in order and
//! stopping after LIMIT rows. A plain `(last_activity_at, created_at, id)`
//! index would NOT match, because the sort key is the COALESCE expression, not
//! the bare column.
//!
//! **Column notes.**
//!
//! - `branch_at_launch` / `repo_root` are a spawn-time snapshot of the local git
//!   branch checked out in `cwd` and the repository root that contained it. Both
//!   are NULL when the launch directory was not inside a git repo (or HEAD was
//!   detached), and on any row written before the columns existed — the
//!   navigator's frontend falls back to the cwd basename then. There is no
//!   backfill: we cannot recover what `git rev-parse` would have reported at the
//!   historical spawn moment.
//! - `requested_workdir` is the user-selected launch directory, before any
//!   worktree resolution. For a worktree-on spawn `cwd` holds the auto-generated
//!   worktree path (under `$DELTA_WORKTREE_BASE`) while this holds the dir the
//!   user actually picked (which is also the worktree's `repo_root`); for a plain
//!   spawn it equals `cwd`. NULL when no workdir was selected (the default
//!   per-token scratch dir) and for sessions that predate the column. The Recent
//!   dirs query groups on `COALESCE(requested_workdir, cwd)` so worktree-managed
//!   paths drop out and legacy rows still appear by their `cwd`.
//! - `repository_display_name` is a spawn-time short repository identity label
//!   (e.g. `org/repo`), derived from the launch directory's `origin` URL and
//!   falling back to the working-tree basename when no origin is configured.
//!   NULL when the launch directory is not a git repo, or for sessions that
//!   predate the column — the navigator renders the cwd basename instead. Stored
//!   separately from `repo_root` because `repo_root` is the working-tree path
//!   (different per linked worktree) while this label is the cross-worktree
//!   repository identity.
//! - `provider` names which AI agent backs the session. `'claude'` for every
//!   session Delta has launched to date (Claude Code in a tmux PTY); other
//!   providers (e.g. `'codex'`, driven via the `codex app-server` JSON-RPC
//!   transport) select a different adapter. `NOT NULL DEFAULT 'claude'` so a row
//!   written before the column existed, and any insert that does not name a
//!   provider, keeps the historical meaning.
//! - `provider_session_id` is the provider's own identifier for the underlying
//!   conversation, when the provider (not Delta) mints it — e.g. Codex's
//!   `thr_...` returned from `thread/start`. NULL for a Claude session, whose
//!   conversation id IS the Delta-minted `session.id` (pinned via
//!   `--session-id`), and for any session that predates the column.
//! - `provider_thread_id` is the provider's thread identifier. A Delta session
//!   maps 1:1 onto a Codex thread, so for Codex this currently equals
//!   `provider_session_id`; it is kept as a distinct column so a future
//!   many-threads-per-session provider has a home for it. NULL for Claude and
//!   for rows that predate the column.
//! - `pull_request_number` is a spawn-time snapshot of the GitHub pull request
//!   the session was opened from — the number the user picked on the new-session
//!   screen's PR tab. Like `branch_at_launch` / `repository_display_name` it is
//!   written once, by the spawning insert, and never updated on resume. NULL for
//!   a session started from the Repository/Directory tab, for a session created
//!   by a hook-registered `claude` that Delta did not spawn (that path knows no
//!   launch context at all), and for every row that predates the column. There
//!   is no backfill: nothing records which PR a historical session came from.
//!   Only the number is stored — the PR's web URL is rebuilt from
//!   `repository_display_name`, which for a PR-picked session names the very
//!   same GitHub repository (Delta's PR listing is `github.com`-only).

use super::Step;

/// The `session` table's history: the v3 baseline table, its recency index, and
/// the v7 pull-request snapshot column.
pub(super) const STEPS: &[Step] = &[
    Step::additive(
        3,
        "\
CREATE TABLE IF NOT EXISTS session (
  id                TEXT PRIMARY KEY,
  cwd               TEXT NOT NULL,
  transcript_path   TEXT,
  title             TEXT,
  status            TEXT NOT NULL
                      CHECK (status IN ('spawning','active','ended','failed')),
  created_at        TEXT NOT NULL,
  last_activity_at  TEXT,
  branch_at_launch  TEXT,
  repo_root         TEXT,
  requested_workdir TEXT,
  repository_display_name TEXT,
  provider TEXT NOT NULL DEFAULT 'claude',
  provider_session_id TEXT,
  provider_thread_id TEXT
) STRICT;",
    ),
    Step::additive(
        3,
        "\
CREATE INDEX IF NOT EXISTS ix_session_recency
  ON session(COALESCE(last_activity_at, created_at) DESC, created_at DESC, id DESC);",
    ),
    // v7: the PR a session was opened from. Nullable with no default, so every
    // existing row reads NULL — "this session was not started from a PR (or
    // predates the column)", which is exactly what the navigator renders as an
    // empty slot.
    Step::additive(
        7,
        "ALTER TABLE session ADD COLUMN pull_request_number INTEGER;",
    ),
];
