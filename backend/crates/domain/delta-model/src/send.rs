//! The outgoing-send queue.
//!
//! Every user input Delta sends to a session's agent is first recorded as a
//! `send` row in the session's correlation FIFO, and moves through `queued` →
//! `dispatched` → `matched` (or `cancelled`) as it is handed to the agent and
//! correlated with what the agent recorded; a send the agent can take at once
//! is written `dispatched`. How each step happens is the
//! provider adapter's business: a Claude send is typed into the tmux pane and
//! matched when its echo arrives (the `UserPromptSubmit` hook and the
//! transcript), while a Codex send rides a `turn/start` request and is matched
//! to the turn that request returns.

use crate::message_uuid::MessageUuid;
use crate::send_status::SendStatus;
use crate::session_id::SessionId;
use crate::thread_id::ThreadId;

/// A recorded user input awaiting correlation with the conversation.
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
    /// Optional short quote injected as hidden context to locate the reply
    /// (Claude: the hook's `additionalContext`; Codex: `thread/inject_items`).
    pub locator_quote: Option<String>,
    pub status: SendStatus,
    /// What the send was correlated with once matched: the uuid of the
    /// transcript line that echoed it for Claude, the turn id `turn/start`
    /// returned for Codex (the provider thread id when it returned none). It
    /// stays `None` on a matched Claude send that was delivered but claimed by
    /// no transcript line, so `None` never means "not delivered".
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
