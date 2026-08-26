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
    /// ISO-8601 timestamp marking this `queued` row as **held in the queue
    /// until the user releases it**. Two paths set it, both recovering a row
    /// that was `dispatched` with no one left to await its echo:
    ///
    /// - the boot-time restore, for a row a dead server process left behind;
    /// - the echo-deadline park, for a row whose keystrokes were swallowed
    ///   without a trace twice running.
    ///
    /// A held send is never dispatched automatically: the queued-dispatch
    /// selection skips rows carrying this marker, so the row stays visible in
    /// the open-send list until the user explicitly releases it (clearing the
    /// marker, returning it to the normal queued flow) or cancels it. `None`
    /// for every row on the normal queued/dispatched path.
    pub held_at: Option<String>,
}
