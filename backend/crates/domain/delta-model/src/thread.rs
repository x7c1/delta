//! Sessions and threads.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::ids::{MessageUuid, SessionId, ThreadId};

/// Lifecycle status of the single Claude Code session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Active,
    Ended,
}

impl SessionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            SessionStatus::Active => "active",
            SessionStatus::Ended => "ended",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "active" => Ok(SessionStatus::Active),
            "ended" => Ok(SessionStatus::Ended),
            other => Err(Error::InvalidVariant {
                kind: "SessionStatus",
                value: other.to_owned(),
            }),
        }
    }
}

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

/// A thread: a subtree of the `to:` reply graph.
///
/// `main` is the trunk thread. Child threads are created when a user branches
/// off an existing message. A thread is identified by its [`ThreadId`] and
/// knows its parent thread and the message its subtree is rooted at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Thread {
    pub id: ThreadId,
    pub session_id: SessionId,
    pub title: String,
    pub parent_thread_id: Option<ThreadId>,
    /// The message this thread branches from, if any.
    pub root_message_uuid: Option<MessageUuid>,
    /// ISO-8601 timestamp.
    pub created_at: String,
}
