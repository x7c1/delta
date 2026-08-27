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
//! accept→launch window (`launching_spawn`, carrying the worktree that launch
//! still has to build, `planned_worktree`), the bind/resume launch state
//! (`spawn`), the turn machine (`turn`), the streaming preview (`streaming`),
//! permission waiters (`permission`), the pending question (`question`),
//! subagent tracking (`subagents`), and the auto-compact debounce
//! (`auto_compact`) — keeps its related types and `impl SessionRuntime` block
//! in its own file.

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

mod launching_spawn;
pub use launching_spawn::LaunchingSpawn;

mod planned_worktree;
pub use planned_worktree::PlannedWorktree;

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
pub use turn::ECHO_DEADLINE;

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
    /// The accepted session whose launch preparation (worktree build, trust
    /// seed, settings write, tmux launch) is still running on a background
    /// task. Present from the moment the first send is accepted until that
    /// task checks in from its `LaunchPrepared` checkpoint, where it becomes a
    /// [`Self::pending_spawn`]; a preparation that fails before that checkpoint
    /// is rolled back from here, with the eager row.
    launching_spawn: Option<LaunchingSpawn>,
    /// The fresh spawn awaiting its first `UserPromptSubmit`/`SessionStart`,
    /// recorded just before its pane is created. At most one exists per
    /// session: each spawn mints a fresh session id.
    pending_spawn: Option<PendingSpawn>,
    /// The resumed-but-not-yet-dispatched launch state, present from
    /// `open_session` until the held prompt dispatches (or the resume fails).
    /// Presence is the "hold sends" flag; absence means ready and dispatched.
    resuming: Option<ResumingSession>,
    /// The session's turn state. [`TurnState::Idle`] when no turn is in flight,
    /// which is also the implicit state of a session with no actor at all.
    turn: TurnState,
    /// When the current [`TurnState::AwaitingEcho`] wait began, and `None`
    /// whenever no send is being awaited — the echo-deadline watchdog's clock.
    ///
    /// Kept in lockstep with [`Self::turn`] by [`SessionRuntime::apply_turn`]
    /// (and restarted by [`SessionRuntime::restamp_awaiting_echo`] on the two
    /// paths that re-type an already-outstanding send). `Instant` is monotonic,
    /// so the elapsed comparison is immune to system-clock changes, matching
    /// the launch deadlines. NOT part of [`Self::is_empty`]: it is only ever
    /// `Some` alongside a non-idle turn, which already pins the actor alive.
    awaiting_echo_since: Option<Instant>,
    /// Oneshot waiters for permission requests the browser may decide, keyed
    /// by request-row id. Registered by the `PermissionRequest` hook (whose
    /// response blocks on the receiver), resolved by a browser decision, and
    /// abandoned on the transport's timeout.
    permission_waiters: HashMap<i64, oneshot::Sender<PermissionDecision>>,
    /// The permission dialogs currently awaiting an answer, oldest first (the
    /// head is the one the browser shows). Unlike a waiter (which only lives
    /// while the hook response blocks on a browser decision), an entry survives
    /// the decision-wait timeout: the TUI prompt is still up, so the question is
    /// still genuinely pending.
    ///
    /// A queue rather than a slot because an adapter-backed provider (Codex) can
    /// raise N approvals within one turn — see
    /// [`SessionRuntime::enqueue_pending_permission`] for what keeping only the
    /// last one cost. A `Vec`, not a `VecDeque`: removals are keyed by request id
    /// rather than strictly from the front, and every reader wants a slice.
    pending_permissions: Vec<PendingPermission>,
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
    /// [`Self::is_empty`] — an undrained entry must not pin the actor alive.
    ///
    /// Not every recorded entry has a drain waiting for it: a NESTED `Agent`
    /// launch (a subagent calling `Agent` itself) reaches the same hook, and its
    /// `tool_use_id` lives only in the subagent's own transcript, so the parent
    /// fold never emits the `SubagentIndicatorStarted` that would take it. It
    /// cannot be told apart when it is recorded — the hook payload carries no
    /// parent/depth signal, and Claude Code 2.1.193 presents such a hook with
    /// the PARENT's `transcript_path`, so the foreign-transcript guard does not
    /// catch it either.
    ///
    /// So the buffer is swept where the agent process is known to be gone:
    /// [`Self::drain_running_subagents`] (the session-close / session-end sweep)
    /// and [`Self::forget_turn`] (session deletion). `close_session` syncs the
    /// transcript immediately before that sweep, so a legitimately pending id
    /// has already had its drain there. `on_session_end` does not sync first —
    /// the background tail can still fold a straggler launch line afterwards,
    /// creating an entry with `task_id: None` — but with the process gone no
    /// `<task-notification>` can follow it either, so the id that fold misses
    /// had nothing left to correlate. Not at turn end: the launch line may still
    /// be missing from the parent JSONL when the turn-end hook fires — that race
    /// is the whole reason this buffer exists.
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
    /// How many times each send has been returned to `queued` by the turn
    /// machine's [`OrphanedSend::Requeue`] disposition, keyed by send id — the
    /// budget that stops a send nobody ever hears about from being re-typed
    /// forever.
    ///
    /// The loop it bounds is dispatch → silence → requeue: no prompt
    /// submission arrives while the send is outstanding, so it is returned to
    /// `queued` and re-typed on the next idle. That is the right answer for a
    /// one-off swallow and the wrong one for keystrokes that vanish the same
    /// way every time, where each attempt burns another model turn.
    /// [`MAX_REQUEUES_PER_SEND`] caps the retries; past the cap the send is
    /// parked instead of requeued — held in the queue for the user's explicit
    /// release, and surfaced. A prompt that does arrive consumes the send by
    /// position whatever its text says, so a rewritten echo spends nothing
    /// here.
    ///
    /// The count does not distinguish *why* the send went back to `queued`.
    /// The [`TurnInput::EchoDeadline`] watchdog is the everyday spender; the
    /// resume window is the other designed-for one — a prompt arriving before
    /// the held keystrokes have been typed at all cannot be the held send's,
    /// so that send returns to the queue. The table's defensive `AwaitingEcho`
    /// arms spend it too: a turn ending, an interrupt, or a fresh dispatch
    /// arriving while a send is still awaiting its echo. That is the point —
    /// the net has to catch failure modes Delta has not thought of — and
    /// firing early is bounded and legible: the send is parked, which leaves
    /// the message in the queue for the user to send or cancel.
    ///
    /// Runtime-only, like everything else here: a restart drops the counts,
    /// granting such a send one further re-dispatch before the cap
    /// stops it again — the loop still terminates, so a retry count persisted
    /// on the send row would not be worth its cost.
    ///
    /// An entry is dropped whenever the turn machine itself retires the send
    /// (a prompt submission consumed it, a slash-command resolution ended its
    /// degenerate turn, orphan-cancelled, parked). A send the *user* cancels
    /// while it sits `queued` leaves its count behind, harmlessly: send ids
    /// are never reused, so a leftover count can never be charged to a later
    /// send. NOT part of [`Self::is_empty`]: a leftover count is a fact about
    /// the past and must not pin the actor alive.
    ///
    /// [`OrphanedSend::Requeue`]: crate::turn::OrphanedSend::Requeue
    /// [`TurnInput::EchoDeadline`]: crate::turn::TurnInput::EchoDeadline
    /// [`MAX_REQUEUES_PER_SEND`]: turn::MAX_REQUEUES_PER_SEND
    requeues_per_send: HashMap<i64, u32>,
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
            && self.launching_spawn.is_none()
            && self.pending_spawn.is_none()
            && self.resuming.is_none()
            && self.turn == TurnState::Idle
            && self.permission_waiters.is_empty()
            && self.pending_permissions.is_empty()
            && self.pending_question.is_none()
            && self.running_subagents.is_empty()
    }
}
