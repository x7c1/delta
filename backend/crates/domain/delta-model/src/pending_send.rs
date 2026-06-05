//! The outgoing-send queue.
//!
//! Every user input Delta sends into the tmux pane is first recorded as a
//! `pending_send`. When a `UserPromptSubmit` hook fires, the server matches the
//! incoming prompt against the head of this FIFO to confirm a turn start and
//! correlate it with the resulting transcript message.

use serde::{Deserialize, Serialize};

use crate::ids::{MessageUuid, SessionId, ThreadId};
use crate::pending_send_status::PendingSendStatus;

/// A queued user input awaiting correlation with the transcript.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingSend {
    pub id: i64,
    pub session_id: SessionId,
    /// The thread this send is attributed to.
    pub thread_id: ThreadId,
    /// When branching, the message this reply is `to:`.
    pub semantic_parent_uuid: Option<MessageUuid>,
    pub text: String,
    /// Optional short quote injected as `additionalContext` to locate the reply.
    pub locator_quote: Option<String>,
    pub status: PendingSendStatus,
    /// The transcript message uuid once matched.
    pub matched_uuid: Option<MessageUuid>,
    /// ISO-8601 timestamp.
    pub created_at: String,
}
