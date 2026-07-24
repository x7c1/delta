//! The runtime state one session actor owns.
//!
//! Everything here is **process-runtime** state, never persisted: after a
//! restart every actor starts from [`SessionRuntime::default`], so every
//! session that survives in the store is considered "closed" (and its turn
//! idle) until it is resumed. One value exists per live actor; absence of an
//! actor reads exactly like this default, which is what makes actor
//! retirement (see the `actor` module) safe.
//!
//! Split into one file per concern: [`SessionRuntime`]'s definition (and its
//! cross-concern `is_empty` retirement predicate) lives here, while each
//! live-state family — the open pane / agent session (`open`, `agent`), the
//! spawn/resume launch state (`spawn`), the turn machine (`turn`), the
//! streaming preview (`streaming`), permission waiters (`permission`), the
//! pending question (`question`), subagent tracking (`subagents`), and the
//! auto-compact debounce (`auto_compact`) — keeps its related types and
//! `impl SessionRuntime` block in its own file.

mod agent;
mod auto_compact;
mod live_state;
mod open;
mod permission;
mod question;
mod spawn;
mod streaming;
mod subagents;
mod turn;

pub use live_state::SessionLiveState;
pub use open::{OpenAgentSession, OpenHandle};
pub use permission::PendingPermission;
pub use question::PendingQuestion;
pub use spawn::{PendingSpawn, ResumingSession, PENDING_SPAWN_DEADLINE, RESUME_READY_DEADLINE};
// Only the deterministic-timing tests read the settle back through the module
// root; production code reaches it inside `spawn` itself, so an unconditional
// re-export would be flagged unused on a non-test build.
#[cfg(test)]
pub use spawn::RESUME_DISPATCH_SETTLE;
pub use streaming::StreamingMessage;
pub use subagents::RunningSubagent;

use std::collections::HashMap;
use std::time::Instant;

use tokio::sync::oneshot;

use crate::agent::AgentContentSource;
use crate::interactor::PermissionDecision;
use crate::turn::TurnState;

/// All of one session's runtime state, owned exclusively by its actor.
///
/// The actor's mailbox is the only way in, so no lock guards any of this: the
/// pane binding, the spawn/resume launch state, the turn state machine, and
/// the pending permission waiters all mutate strictly in mailbox order.
#[derive(Debug, Default)]
pub struct SessionRuntime {
    /// The live pane once the session is bound (open). `None` means closed.
    /// Only ever set for a pane-backed provider (Claude); a terminal-less
    /// provider uses [`Self::open_agent`] instead, so this stays `None` for it.
    open: Option<OpenHandle>,
    /// The live terminal-less agent session (Codex), when open. Mutually
    /// exclusive with [`Self::open`] in practice: a session is pane-backed
    /// (Claude) or adapter-backed (Codex), never both. Kept separate so
    /// Claude's `OpenHandle { token, pane }` path is byte-identical and only
    /// the open-state predicates learn about the new shape.
    open_agent: Option<OpenAgentSession>,
    /// The fresh spawn awaiting its first `UserPromptSubmit`/`SessionStart`.
    /// At most one exists per session: each spawn mints a fresh session id.
    pending_spawn: Option<PendingSpawn>,
    /// The resumed-but-not-yet-dispatched launch state, present from
    /// `open_session` until the held prompt dispatches (or the resume fails).
    /// Presence is the "hold sends" flag; absence means ready and dispatched.
    resuming: Option<ResumingSession>,
    /// The session's turn state. [`TurnState::Idle`] when no turn is in flight,
    /// which is also the implicit state of a session with no actor at all.
    turn: TurnState,
    /// Oneshot waiters for permission requests the browser may decide, keyed
    /// by request-row id. Registered by the `PermissionRequest` hook (whose
    /// response blocks on the receiver), resolved by a browser decision, and
    /// abandoned on the transport's timeout.
    permission_waiters: HashMap<i64, oneshot::Sender<PermissionDecision>>,
    /// The permission dialog currently awaiting an answer, if any. Unlike a
    /// waiter (which only lives while the hook response blocks on a browser
    /// decision), this survives the decision-wait timeout: the TUI prompt is
    /// still up, so the question is still genuinely pending. At most one
    /// exists — `claude` shows one dialog at a time.
    pending_permission: Option<PendingPermission>,
    /// The `AskUserQuestion` tool call currently presenting its options in the
    /// TUI, if any. Set by the `PreToolUse` hook for that tool and cleared when
    /// the correlated `tool_result` resolves its request row or the turn ends.
    /// At most one exists — `claude` shows one question at a time.
    pending_question: Option<PendingQuestion>,
    /// The provisional live preview of the in-flight turn's assistant message,
    /// accumulated from the `MessageDisplay` hook. `None` between turns. Never
    /// persisted: cleared when the turn ends (so the persisted message takes
    /// over), and therefore not part of [`Self::is_empty`] — a turn returning
    /// to idle drops it via [`Self::apply_turn`].
    streaming_message: Option<StreamingMessage>,
    /// The subagents (`Agent`/`Task` tool calls) currently running in this
    /// session's turn, keyed by `tool_use_id` and kept in start order. Each is
    /// added by the `PreToolUse(Agent)` hook. A FOREGROUND entry is removed by
    /// the matching `PostToolUse(Agent)` and the whole foreground set is swept
    /// when the turn returns to idle. A BACKGROUND entry
    /// (`run_in_background: true`) survives both — its immediate `PostToolUse`
    /// is a no-op and the turn-end sweep skips it — and is removed only when its
    /// completion `<task-notification>` is folded (`Effect::SubagentCompleted`).
    /// The set is part of [`Self::is_empty`] (a stuck entry would otherwise pin
    /// the actor alive). Only `Agent`/`Task` flip it — a subagent's nested tool
    /// calls (e.g. its own `Bash`) reach the same hooks but never match.
    running_subagents: Vec<RunningSubagent>,
    /// The `agentId` values observed on `PostToolUse(Agent)` for background
    /// launches whose running entry does not yet exist when the hook fires,
    /// keyed by `tool_use_id`.
    ///
    /// Once the running-subagent indicator moved off the `PreToolUse` hook path
    /// onto the parent-transcript ingest, a top-level background `Agent` launch
    /// became prone to this race: `PreToolUse(Agent)` force-syncs the parent
    /// transcript, but the assistant's `tool_use(Agent)` block has not always
    /// been flushed to the parent's JSONL by the time the hook handler reads
    /// the file; `PostToolUse(Agent)` then arrives carrying `agentId` while no
    /// in-memory running entry exists, so the existing best-effort upgrade
    /// silently drops the id. A later sync eventually folds the launch line
    /// and creates the entry with `task_id: None`, and a `<task-notification>`
    /// missing `<tool-use-id>` (Claude Code 2.1.193 sometimes strips it for
    /// top-level background launches) then has no fallback correlation key —
    /// the indicator stays lit forever.
    ///
    /// This buffer survives the race: `on_post_tool_use` always records the
    /// `agentId` here (entry-or-insert, so a retried hook cannot overwrite the
    /// first value); the `Effect::SubagentIndicatorStarted` arm of
    /// `sync_transcript` drains the buffer when it creates the in-memory entry
    /// and persists the upgrade through the store. The buffer is NOT part of
    /// [`Self::is_empty`] — leaked entries are reclaimed by actor retirement.
    ///
    /// TODO: clean leaked entries on session lifecycle events. A nested
    /// `Agent`'s `PostToolUse` lands here too, but its `tool_use_id` will never
    /// appear in the parent's JSONL (it lives in the subagent's own transcript),
    /// so the entry is never drained. The leak is bounded by the number of
    /// nested `Agent` launches per session — small in practice — but a sweep
    /// keyed off `SubagentCompleted` or session close would tighten the bound.
    pending_post_tool_use_agent_ids: HashMap<String, String>,
    /// The push-based content accumulator for a terminal-less agent session
    /// (Codex): the event pump folds every [`AgentEvent`] from the session's
    /// stream through this to produce the canonical messages each event
    /// completed. `Some` only while a Codex session is open — built at spawn from
    /// the adapter ([`AgentAdapter::content_source`]) and dropped when the agent
    /// session is removed ([`Self::remove_open_agent`]). `None` for a Claude
    /// session (which pulls its content from a transcript, not this push seam)
    /// and between sessions. Not part of [`Self::is_empty`]: it only ever exists
    /// alongside [`Self::open_agent`], which already pins the actor alive.
    ///
    /// [`AgentEvent`]: crate::agent::AgentEvent
    /// [`AgentAdapter::content_source`]: crate::agent::AgentAdapter::content_source
    agent_content_source: Option<Box<dyn AgentContentSource>>,
    /// Correlation between a Delta permission-row id and the adapter-scoped
    /// provider token for a terminal-less agent session (Codex), keyed by the
    /// `i64` row id.
    ///
    /// The event pump allocates a row when it ingests a
    /// [`AgentEvent::PermissionRequested`] and records the row → token pairing
    /// here; the browser-decision path reads it back to translate the row id to
    /// the token it hands [`AgentAdapter::resolve_permission`], and the pump
    /// removes it when the request resolves. The token is opaque to the domain —
    /// stored and forwarded, never interpreted. `None`-valued in effect (empty)
    /// for a Claude session, whose permission decisions never cross the adapter.
    /// Not part of [`Self::is_empty`]: an entry only ever exists alongside
    /// [`Self::open_agent`], which already pins the actor alive.
    ///
    /// [`AgentEvent::PermissionRequested`]: crate::agent::AgentEvent::PermissionRequested
    /// [`AgentAdapter::resolve_permission`]: crate::agent::AgentAdapter::resolve_permission
    agent_permission_tokens: HashMap<i64, String>,
    /// When the most recent auto-compact re-dispatch ran for this session.
    ///
    /// Two paths can drive that re-dispatch — the live
    /// `SessionStart(source=compact)` hook and the ingestion-time
    /// `Effect::AutoCompactFinished` — and on a live session both can fire
    /// for the same compact event within a single tick. This stamp is the
    /// debounce: a second call inside [`AUTO_COMPACT_REDISPATCH_DEBOUNCE`] of
    /// the first returns `false` from [`Self::try_claim_auto_compact_redispatch`]
    /// and the caller skips the second re-type, preventing a double submission.
    /// `Instant` is monotonic, so the comparison is immune to system-clock
    /// changes. Not part of [`Self::is_empty`] — a stamp from a prior
    /// compaction is just a fact about the past and must not pin the actor
    /// alive.
    ///
    /// [`AUTO_COMPACT_REDISPATCH_DEBOUNCE`]: auto_compact::AUTO_COMPACT_REDISPATCH_DEBOUNCE
    last_auto_compact_redispatch_at: Option<Instant>,
}

impl SessionRuntime {
    /// Whether this runtime is indistinguishable from a freshly-built one.
    ///
    /// When true the actor may retire (see the `actor` module): a later
    /// message for the session spawns a new actor whose default state means
    /// exactly the same thing.
    pub fn is_empty(&self) -> bool {
        self.open.is_none()
            && self.open_agent.is_none()
            && self.pending_spawn.is_none()
            && self.resuming.is_none()
            && self.turn == TurnState::Idle
            && self.permission_waiters.is_empty()
            && self.pending_permission.is_none()
            && self.pending_question.is_none()
            && self.running_subagents.is_empty()
    }
}
