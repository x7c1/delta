//! A thread: a subtree of the `to:` reply graph.

use serde::{Deserialize, Serialize};

use crate::ids::{MessageUuid, SessionId, ThreadId};

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
