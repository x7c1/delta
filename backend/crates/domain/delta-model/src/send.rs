//! The outgoing-send queue.
//!
//! Every user input Delta sends into the tmux pane is first recorded as a
//! `send` row. When a `UserPromptSubmit` hook fires, the server matches the
//! incoming prompt against the head of this FIFO to confirm a turn start and
//! correlate it with the resulting transcript message.

use crate::message::MessageUuid;
use crate::send_status::SendStatus;
use crate::session::SessionId;
use crate::thread::ThreadId;

/// A recorded user input awaiting correlation with the transcript.
///
/// Named `Send` after the `send` table it is stored in. The name shadows the
/// `std::marker::Send` prelude trait when imported, so modules that both import
/// this type and spell a `Send` trait bound must qualify the bound as
/// `std::marker::Send`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Send {
    pub id: i64,
    pub session_id: SessionId,
    /// The thread this send is attributed to.
    pub thread_id: ThreadId,
    /// When branching, the message this reply is `to:`.
    pub semantic_parent_uuid: Option<MessageUuid>,
    pub text: String,
    /// Optional short quote injected as `additionalContext` to locate the reply.
    pub locator_quote: Option<String>,
    pub status: SendStatus,
    /// The transcript message uuid once matched.
    pub matched_uuid: Option<MessageUuid>,
    /// ISO-8601 timestamp.
    pub created_at: String,
}
