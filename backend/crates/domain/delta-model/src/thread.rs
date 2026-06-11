//! A thread: a subtree of the `to:` reply graph.

use std::fmt;

use crate::message::MessageUuid;
use crate::session::SessionId;

/// Identifier of a thread (an overlay Delta owns, issued by the store).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ThreadId(pub i64);

impl ThreadId {
    /// Borrow the underlying integer value.
    pub fn value(self) -> i64 {
        self.0
    }
}

impl From<i64> for ThreadId {
    fn from(value: i64) -> Self {
        Self(value)
    }
}

impl fmt::Display for ThreadId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A thread: a subtree of the `to:` reply graph.
///
/// `main` is the trunk thread. Child threads are created when a user branches
/// off an existing message. A thread is identified by its [`ThreadId`] and
/// knows its parent thread and the message its subtree is rooted at.
#[derive(Debug, Clone, PartialEq, Eq)]
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
