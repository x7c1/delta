//! The one input enum a session actor consumes.
//!
//! Every signal that can touch a session's runtime state arrives here, in one
//! mailbox: API commands, Claude Code hook deliveries, browser permission
//! decisions, and the background ticks. Per-session ordering is therefore
//! structural — whatever order these are posted in is the order they execute
//! in — and no lock discipline is needed across them.
//!
//! Variants that produce a result carry a [`Reply`] oneshot; the routing layer
//! awaits it, so the interactor's public methods keep their signatures.

use std::time::Instant;

use delta_model::{Message, MessageUuid, Send, ThreadId};
use tokio::sync::oneshot;

use crate::error::Result;
use crate::interactor::hooks::PermissionWait;
use crate::interactor::lifecycle::FreshSpawn;
use crate::interactor::PermissionDecision;
use crate::pane_token::PaneToken;
use crate::ports::{
    MessageDisplayHook, SessionEndHook, SessionEvent, SessionStartHook, StopHook,
    UserPromptSubmitHook,
};
use super::runtime::SessionLiveState;

/// The reply channel for an input that produces a result.
pub(in crate::interactor) type Reply<R> = oneshot::Sender<Result<R>>;

/// One unit of session work, executed by the session's actor in mailbox order.
pub(in crate::interactor) enum SessionInput {
    // ---- API commands ---------------------------------------------------
    /// Enqueue a user input to a thread of this session (the routing layer
    /// already resolved the thread → session ownership).
    EnqueueToThread {
        thread_id: ThreadId,
        branch_from: Option<MessageUuid>,
        text: String,
        locator_quote: Option<String>,
        reply: Reply<(Send, Vec<SessionEvent>)>,
    },
    /// Spawn this (freshly-minted) session's pane, optionally delivering a
    /// first prompt on the launch command line.
    SpawnFresh {
        first_prompt: Option<String>,
        workdir: Option<String>,
        reply: Reply<FreshSpawn>,
    },
    /// Resume the (closed but known) session.
    OpenSession { reply: Reply<PaneToken> },
    /// Close the session: final sync, kill the pane, drop the binding.
    CloseSession { reply: Reply<()> },
    /// Wipe the residual input of the session's pane, if open.
    ClearInput { reply: Reply<()> },

    // ---- Hook deliveries -------------------------------------------------
    UserPromptSubmit {
        hook: UserPromptSubmitHook,
        reply: Reply<(Vec<SessionEvent>, Option<String>)>,
    },
    Stop {
        hook: StopHook,
        reply: Reply<Vec<SessionEvent>>,
    },
    /// `MessageDisplay`: one chunk of the in-flight turn's assistant message,
    /// streamed live. Accumulated into the session's live preview buffer and
    /// re-broadcast as an `AssistantStreaming` event for the browser.
    MessageDisplay {
        hook: MessageDisplayHook,
        reply: Reply<Vec<SessionEvent>>,
    },
    SessionStart {
        hook: SessionStartHook,
        reply: Reply<Vec<SessionEvent>>,
    },
    SessionEnd {
        hook: SessionEndHook,
        reply: Reply<Vec<SessionEvent>>,
    },
    /// `PreToolUse` only records the request row (no runtime state), but it is
    /// still routed through the mailbox so its write is ordered with this
    /// session's ingestion — the matching `tool_result` resolution must never
    /// observe a not-yet-recorded request.
    PreToolUse {
        tool_name: String,
        tool_input_json: String,
        tool_use_id: String,
        reply: Reply<Vec<SessionEvent>>,
    },
    /// `PermissionRequest`: record the row, register a decision waiter, and
    /// hand the transport the receiver it blocks on.
    PermissionRequest {
        tool_name: String,
        tool_input_json: String,
        reply: Reply<PermissionWait>,
    },

    // ---- Permission decisions ---------------------------------------------
    /// The browser answered a pending permission request of this session.
    DecidePermission {
        request_id: i64,
        decision: PermissionDecision,
        reply: Reply<Vec<SessionEvent>>,
    },
    /// The transport's decision wait timed out; drop the waiter (the row stays
    /// `pending` and the TUI prompt owns the gating). Fire-and-forget.
    AbandonPermission { request_id: i64 },

    // ---- Question answers --------------------------------------------------
    /// The browser answered a pending `AskUserQuestion` of this session: inject
    /// the selection keystrokes (built from `selections`) into the TUI pane.
    AnswerQuestion {
        request_id: i64,
        /// The chosen 0-based option index(es) per question, in question order.
        selections: Vec<Vec<usize>>,
        reply: Reply<()>,
    },
    /// The browser cancelled a pending `AskUserQuestion` of this session: inject
    /// `Escape` into the TUI pane, which cancels the whole call.
    CancelQuestion {
        request_id: i64,
        reply: Reply<()>,
    },

    // ---- Background ticks --------------------------------------------------
    /// Poll this session's transcript for newly-written lines (the continuous
    /// tail). A no-op for a session with no live pane.
    SyncTick {
        reply: Reply<(Vec<Message>, Vec<SessionEvent>)>,
    },
    /// Dispatch the held first prompt if this session's resume is ready and
    /// has settled as of `now`.
    ResumeTick { now: Instant, reply: Reply<()> },
    /// Reap this session's launch (fresh spawn or resume) if it never became
    /// ready before its deadline as of `now`.
    ReapTick {
        now: Instant,
        reply: Reply<Vec<SessionEvent>>,
    },

    // ---- Queries (runtime reads) -------------------------------------------
    /// The pane driving the session, if open (the PTY bridge's routing key).
    QueryPane { reply: oneshot::Sender<Option<String>> },
    /// Whether the session is open (has a live, bound pane).
    QueryIsOpen { reply: oneshot::Sender<bool> },
    /// Whether any pane is live for the session (bound, or spawned and
    /// awaiting its first hook). Drives the cold-start idempotence check.
    QueryIsLive { reply: oneshot::Sender<bool> },
    /// The session's queryable live state — the turn phase plus the pending
    /// permission dialog, snapshotted in one message so the sends envelope
    /// reports a consistent pair.
    QueryLiveState {
        reply: oneshot::Sender<SessionLiveState>,
    },

    /// Test seam: run a closure against the session's runtime state, in
    /// mailbox order like any other input. Replaces the lock-era seams that
    /// reached into the shared registries directly.
    #[cfg(test)]
    WithRuntime(Box<dyn FnOnce(&mut super::runtime::SessionRuntime) + std::marker::Send>),
}
