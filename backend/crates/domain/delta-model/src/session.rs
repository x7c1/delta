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
    pub transcript_path: String,
    pub title: Option<String>,
    pub status: SessionStatus,
    /// ISO-8601 timestamp.
    pub created_at: String,
}
