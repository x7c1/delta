//! The single Claude Code TUI session Delta wraps.

use serde::{Deserialize, Serialize};

use crate::ids::SessionId;
use crate::session_status::SessionStatus;

/// The single Claude Code TUI session Delta wraps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub id: SessionId,
    pub cwd: String,
    pub transcript_path: String,
    pub title: Option<String>,
    pub status: SessionStatus,
    /// ISO-8601 timestamp.
    pub created_at: String,
}
