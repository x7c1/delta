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
}
