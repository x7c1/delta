//! The single Claude Code TUI session Delta wraps.

use crate::agent_provider::AgentProvider;
use crate::newtype::string_newtype;
use crate::session_status::SessionStatus;

string_newtype! {
    /// Identifier of the single Claude Code session (`session_id` from hooks).
    SessionId
}

/// The single Claude Code TUI session Delta wraps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub id: SessionId,
    pub cwd: String,
    /// Path of the session's JSONL transcript. `None` while the session is
    /// still [`SessionStatus::Spawning`]: the path is owned by Claude Code and
    /// only learned from the first hook, which fills it as it activates the
    /// session.
    pub transcript_path: Option<String>,
    pub title: Option<String>,
    pub status: SessionStatus,
    /// ISO-8601 timestamp.
    pub created_at: String,
    /// Local branch checked out in [`Self::cwd`] at the moment this session
    /// was spawned (`git rev-parse --abbrev-ref HEAD`). `None` when the launch
    /// directory was not inside a git repository, when HEAD was detached, or
    /// for sessions that predate this field. This is a **spawn-time snapshot**:
    /// it is never mutated on resume or after a later `git checkout` inside
    /// the worktree. The per-message `git_branch` on [`crate::Message`] is a
    /// separate per-turn snapshot and is unaffected.
    pub branch_at_launch: Option<String>,
    /// Working tree root (`git rev-parse --show-toplevel`) of the directory this
    /// session was launched **against**, at spawn time. Like
    /// [`Self::branch_at_launch`], a spawn-time snapshot — never updated later.
    ///
    /// Which directory that is depends on the spawn:
    ///
    /// - a **worktree** spawn resolves it against the dir the user picked, i.e.
    ///   *before* the worktree is created, so it holds the repository the
    ///   worktree was cut from — not the worktree itself (which is
    ///   [`Self::cwd`]). That is what makes it the source of truth for
    ///   re-establishing a worktree session's context on resume;
    /// - a plain spawn resolves it against the launch directory itself, so it
    ///   equals that directory's working-tree top.
    ///
    /// `None` when the launch directory was not inside a git repository, and for
    /// sessions that predate this field. On an adapter-backed (terminal-less)
    /// session it is additionally `None` for **every** non-worktree spawn, git
    /// repository or not, because that path records no repo columns at all — so
    /// a non-NULL value on such a session means exactly "this session runs in a
    /// worktree Delta cut from that repository".
    ///
    /// Note: this is a *working tree* top, not the repository identity. When a
    /// plain spawn's launch directory is itself a linked git worktree,
    /// `--show-toplevel` returns that worktree's path, not the original clone.
    /// Use [`Self::repository_display_name`] for the repository-level identity
    /// label the navigator renders.
    pub repo_root: Option<String>,
    /// The user-selected launch directory, before any worktree resolution.
    /// For a worktree-on spawn this holds the dir the user picked (the
    /// worktree's repo root) while [`Self::cwd`] holds the auto-generated
    /// worktree path. For a plain spawn with a user-selected workdir this
    /// equals [`Self::cwd`]. `None` when no workdir was selected (the default
    /// per-token scratch dir is used) and for sessions that predate this
    /// field. The Recent dirs picker prefers this value, falling back to
    /// [`Self::cwd`] for legacy rows.
    pub requested_workdir: Option<String>,
    /// Short repository identity label captured at spawn time, sourced from
    /// the launch directory's remote `origin` URL. The navigator renders this
    /// directly as the session card's repo line.
    ///
    /// - `Some("org/repo")` when the launch directory's repo has an `origin`
    ///   URL (SSH or HTTPS, normalised to a `host/org/repo` identity key and
    ///   shortened to the `org/repo` tail).
    /// - `Some(basename(repo_root))` when the launch directory is a git repo
    ///   but has no `origin` configured — a local-only clone falls back to
    ///   the working-tree basename so it is still identifiable.
    /// - `None` when the launch directory is not a git repository at all, OR
    ///   for sessions that predate this column (existing rows read `NULL`).
    ///
    /// This is a **spawn-time snapshot**: it is set once when the session
    /// row is first inserted and never updated on resume. Unlike
    /// [`Self::repo_root`], the value is stable across worktrees of the same
    /// clone — the `origin` URL lives in the shared `.git/config`, so a
    /// session launched in a linked worktree carries the same label as one
    /// launched in the main tree.
    pub repository_display_name: Option<String>,
    /// Which AI-agent backend drives this session. [`AgentProvider::Claude`]
    /// for every session Delta launched before multi-provider support, and for
    /// any row that predates the `session.provider` column (it reads the
    /// column's `'claude'` default). The core keys behaviour off the provider's
    /// capabilities, never off this value directly.
    pub provider: AgentProvider,
    /// The provider's own identifier for the underlying conversation, when the
    /// provider — not Delta — mints it (e.g. Codex's `thr_...` returned from
    /// `thread/start`). `None` for a Claude session, whose conversation id *is*
    /// the Delta-minted [`Self::id`], and for any row that predates the column.
    pub provider_session_id: Option<String>,
    /// The provider's thread identifier. A Delta session maps 1:1 onto a
    /// provider thread, so for Codex this currently equals
    /// [`Self::provider_session_id`]; it is kept distinct so a future
    /// many-threads-per-session provider has a home for it. `None` for Claude
    /// and for rows that predate the column.
    pub provider_thread_id: Option<String>,
    /// The GitHub pull request this session was opened from — the number the
    /// user picked on the new-session screen's PR tab.
    ///
    /// A **spawn-time snapshot**, like [`Self::branch_at_launch`] and
    /// [`Self::repository_display_name`]: written once when the session row is
    /// first inserted and never updated on resume. `None` for a session started
    /// from the Repository/Directory tab, for a session an external
    /// (hook-registered) `claude` created — that path knows no Delta launch
    /// context — and for any row that predates the column.
    ///
    /// Only the number is stored. Delta's PR flow is `github.com`-only, and a
    /// PR-picked session's [`Self::repository_display_name`] names the very
    /// repository the PR lives in, so the PR's web URL is rebuilt from the two
    /// where it is rendered rather than persisted alongside.
    pub pull_request_number: Option<i64>,
}
