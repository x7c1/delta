//! The single Claude Code TUI session Delta wraps.

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
    /// Working tree root that contained [`Self::cwd`] at spawn time
    /// (`git rev-parse --show-toplevel`). `None` when the launch directory was
    /// not inside a git repository, or for sessions that predate this field.
    /// Like [`Self::branch_at_launch`], this is a spawn-time snapshot — never
    /// updated later.
    ///
    /// Note: this is the *working tree* top, not the repository identity.
    /// When the launch directory is a linked git worktree, `--show-toplevel`
    /// returns the worktree path itself (e.g.
    /// `$HOME/.delta/worktrees/delta-<id>`), not the original clone. Use
    /// [`Self::repository_display_name`] for the repository-level identity
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
}
