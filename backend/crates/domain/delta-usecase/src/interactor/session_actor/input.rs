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

use delta_model::{AgentProvider, Message, MessageUuid, Send, ThreadId};
use tokio::sync::oneshot;

use super::runtime::SessionLiveState;
use crate::agent::AgentEvent;
use crate::error::Result;
use crate::interactor::hooks::PermissionWait;
use crate::interactor::lifecycle::{FreshSpawn, LaunchApproval};
use crate::interactor::PermissionDecision;
use crate::ports::{
    MessageDisplayHook, SessionEndHook, SessionEvent, SessionStartHook, StopHook,
    UserPromptSubmitHook,
};
use crate::send_target::WorktreeSpec;

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
        /// The user-selected registered launch options to resolve to argv flags
        /// at spawn, in selection order.
        launch_option_ids: Vec<i64>,
        /// An opt-in request to start the session inside a git worktree of the
        /// selected `workdir`. Only meaningful when `workdir` is `Some` and that
        /// directory is a git repository.
        worktree: Option<WorktreeSpec>,
        /// The AI-agent backend to launch on. [`AgentProvider::Claude`] takes
        /// the historical tmux + hooks path (`spawn_fresh`); every other
        /// provider (e.g. [`AgentProvider::Codex`]) takes the terminal-less
        /// adapter path (`spawn_adapter_session`).
        provider: AgentProvider,
        reply: Reply<FreshSpawn>,
    },
    /// The background launch preparation of a freshly-accepted session reached
    /// its last step: everything but the tmux pane is in place, and the pane is
    /// about to be created. The launch task awaits this reply before calling
    /// `create_session`.
    ///
    /// `SpawnFresh` above only *accepts* the session (validating, writing the
    /// eager row and first send, and replying with real ids); the expensive
    /// part — building the git worktree, seeding workspace trust, writing the
    /// settings file and launching `claude` in a tmux pane — runs afterwards on
    /// a `tokio::spawn`ed task holding a weak handle to this mailbox, exactly
    /// like the Codex event pump. This is that task's checkpoint partway
    /// through.
    ///
    /// Its handler turns the launching entry into the `PendingSpawn` that the
    /// launch's first `SessionStart`/`UserPromptSubmit` binds. That record must
    /// exist *before* the pane does: those hooks land on this same mailbox, so
    /// a pane created first can fire one that finds nothing pending, is
    /// dismissed as external input, and leaves the spawn recorded afterwards
    /// with no hook left to bind it — a session that never activates. Awaiting
    /// this reply makes the ordering structural instead of a race the launch
    /// usually wins.
    ///
    /// The reply also tells the task whether to go on at all:
    /// [`LaunchApproval::Abandon`] when no launching entry matches `token` (the
    /// acceptance was already rolled back), in which case no pane is created.
    /// See `SessionContext::record_launched_pane`.
    LaunchPrepared {
        token: crate::pane_token::PaneToken,
        reply: Reply<LaunchApproval>,
    },
    /// The background launch of a freshly-accepted **adapter-backed** session
    /// (Codex) has connected its provider and started its thread, and hands the
    /// live connection over to be bound.
    ///
    /// The terminal-less counterpart of `LaunchPrepared`, and its mirror image:
    /// a pane launch checks in before its pane exists, an adapter launch after
    /// its connection does. What the handler does with it (persist the provider
    /// ids, bind the adapter, start the event pump, dispatch the accepted first
    /// prompt, announce `session_registered`) — and why the two check in at
    /// opposite ends — is documented on `SessionContext::bind_adapter_launch`
    /// and its module.
    ///
    /// The reply is fallible, unlike `LaunchPrepared`'s: binding persists rows
    /// and dispatches a turn, so its error becomes the launch's error and rides
    /// out on the `SpawnFailed` the browser shows. A
    /// [`LaunchApproval::Abandon`] answer means the acceptance was already
    /// rolled back, in which case the task drops the connection it was holding
    /// rather than leaving a provider process behind.
    ///
    /// Boxed because it is by far the largest variant here (a live adapter, a
    /// provider handle and the observed branch), and every `SessionInput` would
    /// otherwise be sized by it.
    AdapterLaunchPrepared {
        token: crate::pane_token::PaneToken,
        prepared: Box<crate::interactor::lifecycle::PreparedAdapterLaunch>,
        reply: Reply<LaunchApproval>,
    },
    /// The background launch preparation of a freshly-accepted session
    /// finished, reported by the launch task through this actor's own mailbox.
    ///
    /// The counterpart to `LaunchPrepared` above, and unlike it fire-and-forget:
    /// the task has no caller to reply to, so a failure is reported on the async
    /// event seam instead.
    ///
    /// `token` identifies the launch the outcome belongs to, so a report that
    /// arrives after its entry was already rolled back is discarded rather than
    /// applied to an unrelated launch. What each outcome does to the session —
    /// `Ok` leaves the already-recorded pending spawn alone, `Err` rolls the
    /// acceptance back and reports it on the async event seam — is documented on
    /// the handler, `SessionContext::finish_launch`.
    LaunchFinished {
        token: crate::pane_token::PaneToken,
        outcome: Result<()>,
    },
    /// Resume the (closed but known) session.
    OpenSession { reply: Reply<()> },
    /// Close the session: final sync, kill the pane, drop the binding. Replies
    /// with the [`SessionEvent::SubagentFinished`]s the process-gone sweep
    /// produced, for the transport to broadcast.
    CloseSession { reply: Reply<Vec<SessionEvent>> },
    /// Interrupt the session's in-flight turn without closing it: reach the open
    /// agent and drive [`AgentAdapter::interrupt`], leaving the open agent (and
    /// its event pump) in place so the provider's `turn/completed{interrupted}`
    /// can arrive and settle the turn. A well-defined no-op for a pane-backed
    /// (Claude) or closed session — those carry no open agent.
    ///
    /// [`AgentAdapter::interrupt`]: crate::agent::AgentAdapter::interrupt
    Interrupt { reply: Reply<()> },
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
        /// The JSONL the hook is firing against. Compared against the session
        /// row's `transcript_path` so a hook fired by a nested subagent (whose
        /// own transcript differs from the parent's) can be short-circuited
        /// before it leaks runtime state onto the parent.
        transcript_path: String,
        reply: Reply<Vec<SessionEvent>>,
    },
    /// `PostToolUse`: a tool call completed. Delta acts on the subagent
    /// (`Agent`/`Task`) case in two ways — for a foreground subagent it closes
    /// the running window by `tool_use_id`; for a background subagent it
    /// upgrades the matching launch row with the `agentId` reported in
    /// `tool_response_json` so the eventual `<task-notification>` has a
    /// fallback correlation key. Routed through the mailbox so the
    /// clear/upgrade is ordered with the `PreToolUse` that opened the window.
    PostToolUse {
        tool_name: String,
        tool_use_id: String,
        /// The `tool_response` content carried by the hook payload, serialized
        /// as JSON text. Read by the subagent handler for an `agentId` field;
        /// otherwise unused. Always a parseable JSON string (`"null"` or `"{}"`
        /// when the hook carried nothing useful) so the consumer can `from_str`
        /// without first handling an absent shape.
        tool_response_json: String,
        /// The JSONL the hook is firing against. Compared against the session
        /// row's `transcript_path` so a hook fired by a nested subagent (whose
        /// own transcript differs from the parent's) can be short-circuited
        /// before it touches the parent's running window.
        transcript_path: String,
        reply: Reply<Vec<SessionEvent>>,
    },
    /// `PermissionRequest`: record the row, register a decision waiter, and
    /// hand the transport the receiver it blocks on.
    PermissionRequest {
        tool_name: String,
        tool_input_json: String,
        /// The JSONL the hook is firing against. Compared against the session
        /// row's `transcript_path` so a permission dialog raised inside a
        /// nested subagent does not register a parent-attributed waiter.
        transcript_path: String,
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
    CancelQuestion { request_id: i64, reply: Reply<()> },

    // ---- Send cancellation -------------------------------------------------
    /// The browser cancelled a still-`queued` send of this session before it was
    /// dispatched: transition the row to `cancelled` so the idle dispatch path
    /// skips it. A no-op conflict if the send already left `queued`.
    CancelSend { send_id: i64, reply: Reply<()> },
    /// The browser released a *held* send of this session (recovered at boot
    /// from a dead process's `dispatched` state, or parked by the echo
    /// deadline): clear its hold marker so it re-enters the normal queued
    /// flow, then run the idle dispatch. Replies with the
    /// [`SessionEvent::SendDispatched`] to broadcast when that dispatch
    /// promoted a send. A conflict if the send is not a still-queued held
    /// row.
    ///
    /// [`SessionEvent::SendDispatched`]: crate::ports::SessionEvent::SendDispatched
    ReleaseSend {
        send_id: i64,
        reply: Reply<Option<SessionEvent>>,
    },

    // ---- Agent event pump --------------------------------------------------
    /// One neutral [`AgentEvent`] from a terminal-less agent session's event
    /// stream (Codex), delivered by that session's event pump.
    ///
    /// Routed through the same mailbox as every other signal so it is ordered
    /// against this session's other work — content persistence and the turn
    /// machine mutate in event-arrival order, and a `TurnCompleted` lands after
    /// the messages of the turn it ends. Fire-and-forget: the pump is a
    /// background drain with no caller to reply to. The handler folds the event
    /// through the session's content accumulator (persisting any completed
    /// messages), advances the turn machine on turn-end, and emits the resulting
    /// browser events on the async seam ([`InteractorCore::emit_async_event`]) —
    /// the event arrives after the driving `enqueue`/spawn call already returned,
    /// so there is no synchronous `Vec<SessionEvent>` to fold it into.
    ///
    /// [`InteractorCore::emit_async_event`]: crate::interactor::InteractorCore::emit_async_event
    IngestAgentEvent { event: AgentEvent },

    // ---- Background ticks --------------------------------------------------
    /// Poll this session's transcript for newly-written lines (the continuous
    /// tail). A no-op for a session with no live pane.
    SyncTick {
        reply: Reply<(Vec<Message>, Vec<SessionEvent>)>,
    },
    /// Dispatch the held first prompt if this session's resume is ready and
    /// has settled as of `now` — or, when the settled resume held no prompt,
    /// flush the session's oldest `queued` send (deferred by the resume
    /// window). Replies with the [`SessionEvent::SendDispatched`] to
    /// broadcast when that flush promoted a send.
    ResumeTick {
        now: Instant,
        reply: Reply<Option<SessionEvent>>,
    },
    /// Reap this session's launch (fresh spawn or resume) if it never became
    /// ready before its deadline as of `now`.
    ReapTick {
        now: Instant,
        reply: Reply<Vec<SessionEvent>>,
    },
    /// Give up on this session's outstanding send if its `UserPromptSubmit`
    /// echo — and every other signal — has failed to arrive by its deadline as
    /// of `now`, retrying or parking it and flushing the queue behind it.
    /// Replies with the [`SessionEvent::SendDispatched`] to broadcast when that
    /// flush promoted a send.
    ///
    /// [`SessionEvent::SendDispatched`]: crate::ports::SessionEvent::SendDispatched
    EchoDeadlineTick {
        now: Instant,
        reply: Reply<Option<SessionEvent>>,
    },

    // ---- Queries (runtime reads) -------------------------------------------
    /// The pane driving the session, if open (the PTY bridge's routing key).
    QueryPane {
        reply: oneshot::Sender<Option<String>>,
    },
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
