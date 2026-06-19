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
    /// Repository root that contained [`Self::cwd`] at spawn time
    /// (`git rev-parse --show-toplevel`). `None` when the launch directory was
    /// not inside a git repository, or for sessions that predate this field.
    /// Like [`Self::branch_at_launch`], this is a spawn-time snapshot — never
    /// updated later.
    pub repo_root: Option<String>,
}
