//! The runtime state one session actor owns.
//!
//! Everything here is **process-runtime** state, never persisted: after a
//! restart every actor starts from [`SessionRuntime::default`], so every
//! session that survives in the store is considered "closed" (and its turn
//! idle) until it is resumed. One value exists per live actor; absence of an
//! actor reads exactly like this default, which is what makes actor
//! retirement (see the `actor` module) safe.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::oneshot;

use delta_model::ThreadId;

use crate::agent::{AgentAdapter, AgentSessionHandle};
use crate::interactor::PermissionDecision;
use crate::pane_token::PaneToken;
use crate::turn::{transition, Transition, TurnInput, TurnState};

/// How long a spawn may sit unbound before the watchdog reaps it.
///
/// A spawn binds the instant its first `UserPromptSubmit` hook fires, which on a
/// healthy launch happens within a second or two of `claude` reaching its
/// prompt. This deadline is set deliberately generous — far longer than even a
/// slow cold start — so a genuinely-slow-but-healthy launch always binds first
/// via the normal hook path and the reaper only ever catches a spawn that is
/// truly stuck (crashed/exited/hung before it could register). The `SessionEnd`
/// hook is the precise early signal for an exited launch; this deadline is the
/// coarse backstop for the hang-forever case the hook cannot observe.
pub const PENDING_SPAWN_DEADLINE: Duration = Duration::from_secs(30);

/// How long a resumed session may sit not-ready before the watchdog fails it.
///
/// `claude --resume <id>` is event-driven: Delta binds the pane immediately but
/// holds the first prompt until the session's `SessionStart` (`source=resume`)
/// hook signals the TUI is ready to accept input (measured ~2s after launch on
/// a healthy resume). If that hook never arrives — the resume crashed, hung on
/// auth, or failed to replay its transcript after the existence gate — nothing
/// else would release the held prompt and the UI is stuck "pending" forever.
/// This deadline is the backstop: a resume still not-ready past it is failed
/// (pane killed, held prompt cancelled, `SpawnFailed` emitted). It is set
/// generously above the observed readiness latency so a slow-but-healthy resume
/// always becomes ready first via the hook. The `SessionEnd` hook is the precise
/// early signal for an exited resume; this is the coarse backstop for the
/// hang-forever case the hook cannot observe. Mirrors [`PENDING_SPAWN_DEADLINE`]
/// for fresh spawns.
pub const RESUME_READY_DEADLINE: Duration = Duration::from_secs(30);

/// How long after a resume is marked ready Delta waits before dispatching its
/// held first prompt.
///
/// `SessionStart(source=resume)` is delivered as a hook that blocks `claude`
/// until the hook's HTTP handler returns. So the handler must not type the held
/// prompt itself: while it is inside the hook, `claude` has not yet returned to
/// its prompt and is not accepting input, and any keystroke sent then is lost
/// (no `UserPromptSubmit` fires, the prompt never submits). Instead the handler
/// only *marks the resume ready*; the keystroke is dispatched later, off the
/// background tick, after the hook has returned and `claude` is input-ready.
///
/// This small settle is the margin between "ready was marked" and "dispatch the
/// keystroke": the tick only dispatches a ready resume once `now - ready_at` has
/// reached this value, so the dispatch is guaranteed to run a beat after the
/// hook returned rather than racing it. Kept short (the hook has already
/// returned by the time the tick runs) but non-zero so the ordering is
/// deterministic. Compared against an injected `now`, like the watchdog
/// deadlines, so it is testable without wall-clock sleeps.
pub const RESUME_DISPATCH_SETTLE: Duration = Duration::from_millis(200);

/// How long after an auto-compact re-dispatch fires for a session before
/// another re-dispatch may run for the same session.
///
/// Two paths drive auto-compact re-dispatch — the live
/// `SessionStart(source=compact)` hook and the ingestion-time
/// `Effect::AutoCompactFinished` from the same compaction summary line — and
/// on a live session both can land within a single tick. Without a debounce
/// each `Dispatched` send would be re-typed twice, producing a spurious
/// double submission. Set generously above the gap between the hook and the
/// ingest (the hook fires when Claude finishes compacting; the tail ingests
/// the summary line on the next poll) but well under any plausible interval
/// between distinct compactions.
pub const AUTO_COMPACT_REDISPATCH_DEBOUNCE: Duration = Duration::from_secs(2);

/// A live, bound session: its Claude `session_id` is known and it is mapped to
/// the tmux pane driving it.
#[derive(Debug, Clone)]
pub struct OpenHandle {
    /// The Delta-minted tmux session name.
    pub token: PaneToken,
    /// The pane keystrokes are sent to and the PTY attaches to (`<token>:0.0`).
    pub pane: String,
}

/// A live, terminal-less agent session (e.g. Codex over `codex app-server`).
///
/// The parallel of [`OpenHandle`] for a provider that has no tmux pane: it
/// carries the live [`AgentAdapter`] and the provider's session handle instead
/// of a pane token. Holding the adapter here is what keeps the underlying
/// `codex app-server` connection alive for the session's lifetime — dropping it
/// (e.g. on actor retirement) would tear the connection down — so a session
/// with an `open_agent` reads as *open* and its actor never retires while it
/// exists (see [`SessionRuntime::is_empty`]).
///
/// There is deliberately no [`OpenHandle`] for such a session, so Claude's
/// pane-bound path is untouched: [`SessionRuntime::handle`] (the PTY routing
/// key) stays `None`, and the PTY bridge therefore refuses to attach — a Codex
/// session has nothing to attach to ([`crate::agent::TerminalCapability::NoTerminal`]).
#[derive(Clone)]
pub struct OpenAgentSession {
    /// The live adapter driving the provider. Kept alive here for the session's
    /// lifetime so its backing connection is not dropped underneath it.
    pub adapter: Arc<dyn AgentAdapter>,
    /// The provider's handle for this session (its provider session id + the
    /// adapter-local key), used to address the session on the adapter.
    pub handle: AgentSessionHandle,
}

impl std::fmt::Debug for OpenAgentSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The adapter is a trait object with no `Debug`; print the handle and
        // the provider it drives, which is the identifying state anyway.
        f.debug_struct("OpenAgentSession")
            .field("provider", &self.adapter.provider())
            .field("handle", &self.handle)
            .finish_non_exhaustive()
    }
}

/// A freshly-spawned pane awaiting its first `UserPromptSubmit`.
///
/// Delta pins the conversation's `session_id` at spawn time by passing a
/// freshly-minted UUID to `claude --session-id <uuid>`, so the first
/// `UserPromptSubmit` hook reports exactly that id — which routes the hook to
/// this spawn's actor, whose pending entry this is. The session row (and any
/// first prompt's send row) is written eagerly at spawn time, so binding only
/// activates that row and maps the pane.
#[derive(Debug, Clone)]
pub struct PendingSpawn {
    /// The Delta-minted tmux session name.
    pub token: PaneToken,
    /// The pane keystrokes are sent to (`<token>:0.0`).
    pub pane: String,
    /// When this spawn was recorded, for the watchdog deadline.
    ///
    /// A spawn is fire-and-forget: only the first `UserPromptSubmit` hook binds
    /// it, so a launch that crashes/hangs before that hook never times out on
    /// its own. The reaper compares `now - created_at` against
    /// [`PENDING_SPAWN_DEADLINE`] to detect and clean up such a stuck spawn.
    /// `Instant` is monotonic, so it measures elapsed wall time without being
    /// perturbed by system-clock changes.
    pub created_at: Instant,
}

/// A resumed-but-not-yet-ready session: its pane is bound, but its first prompt
/// is held until the resume's `SessionStart` (`source=resume`) hook arrives and,
/// crucially, until a beat *after* that hook returns.
///
/// Unlike a fresh spawn, a resume's `session_id` is known up front, so the pane
/// binds immediately. But `claude --resume` needs a couple of seconds to replay
/// the transcript and make its TUI input ready, far longer than any fixed
/// settle could safely cover. So Delta does not type the first prompt at
/// resume time; it records the resume here.
///
/// Readiness arrives in two stages, tracked by [`Self::ready_at`]:
///
/// 1. **Mark ready** — `SessionStart(source=resume)` fires. That hook blocks
///    `claude` until its HTTP handler returns, so the handler must *not* type
///    the held prompt (the keystroke would land while `claude` is still inside
///    the hook and not accepting input, and be lost). It only stamps
///    [`Self::ready_at`] and returns immediately, unblocking `claude`.
/// 2. **Dispatch** — on a later background tick, once `now - ready_at` has
///    reached [`RESUME_DISPATCH_SETTLE`], the held keystroke is dispatched on
///    the normal `send_line` path. By then the hook has returned and `claude`
///    is input-ready, so the keystroke submits.
///
/// The `send` row for that first prompt is written normally (its thread,
/// branch, and locator-quote semantics are persisted up front) — only the
/// physical keystroke is held.
#[derive(Debug, Clone)]
pub struct ResumingSession {
    /// The Delta-minted tmux session name backing the resumed pane, killed by
    /// the watchdog if the resume never becomes ready.
    pub token: PaneToken,
    /// The pane the held first prompt is dispatched into once ready.
    pub pane: String,
    /// The held first prompt's keystroke text, if a send is waiting on this
    /// resume's readiness. A resume opened with no immediate send (e.g. an
    /// explicit `open_session` with no following dispatch) carries `None`.
    pub held_prompt: Option<String>,
    /// When the resume was recorded, for the readiness watchdog deadline. Like
    /// [`PendingSpawn::created_at`] this is a monotonic `Instant`, so the
    /// deadline check is immune to system-clock changes.
    pub created_at: Instant,
    /// When `SessionStart(source=resume)` marked this resume ready, or `None`
    /// while still not-ready.
    ///
    /// `None` means the readiness hook has not arrived: the held prompt is
    /// parked and the watchdog may reap the resume if [`RESUME_READY_DEADLINE`]
    /// passes. `Some(t)` means the hook fired at `t` and the resume is pending
    /// dispatch — the watchdog must leave it alone, and the dispatch tick types
    /// the held prompt once `now - t` reaches [`RESUME_DISPATCH_SETTLE`].
    pub ready_at: Option<Instant>,
}

/// A permission dialog currently awaiting a human answer — in the browser
/// (the notice's Allow/Deny) or in the TUI prompt after the browser-decision
/// wait timed out.
///
/// This is the queryable counterpart of the `PermissionRequested` broadcast:
/// the event is lost for a client whose socket was down when it fired, so the
/// sends envelope (`GET /api/sessions/{id}/sends`) reports this state and a
/// reconnecting client rebuilds its notice from a plain refetch, exactly like
/// the turn state. Cleared when the request resolves (a browser decision or
/// the correlated `tool_result`) and whenever the turn returns to idle — a
/// dialog cannot outlive its turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingPermission {
    /// The `permission_request` row id (the decision endpoint's key).
    pub request_id: i64,
    pub tool_name: String,
    /// The tool input, serialized as JSON text.
    pub tool_input_json: String,
}

/// An `AskUserQuestion` tool call currently presenting its options in the TUI,
/// awaiting the user's pick.
///
/// The queryable counterpart of the `QuestionAsked` broadcast, mirroring
/// [`PendingPermission`]: the event is lost for a client whose socket was down
/// when it fired, so the sends envelope reports this state and a reconnecting
/// client rebuilds its question card from a plain refetch. Cleared when the
/// correlated `tool_result` resolves the request (the user answered in the TUI)
/// and whenever the turn returns to idle — a question cannot outlive its turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingQuestion {
    /// The `PreToolUse` row id that recorded this question (its `tool_use_id`
    /// is what the later `tool_result` resolves it by).
    pub request_id: i64,
    /// The in-flight turn's thread, so the browser only shows the question card
    /// on the thread it belongs to.
    pub thread_id: ThreadId,
    /// The raw `{"questions":[…]}` tool input, serialized as JSON text, which
    /// the browser parses to render the question card.
    pub tool_input_json: String,
}

/// A subagent (the `Agent`/`Task` tool) currently running inside the session's
/// main turn.
///
/// A subagent runs in its own transcript that Delta never tails, so the main
/// conversation pane shows nothing while it works. This is the queryable
/// counterpart of the `subagent_started`/`subagent_finished` broadcasts: those
/// events are lost for a client whose socket was down when they fired, so the
/// sends envelope reports the running set and a reconnecting client rebuilds
/// its indicator from a plain refetch — exactly like [`PendingPermission`] and
/// [`PendingQuestion`].
///
/// A foreground subagent's running window is the synchronous
/// `PreToolUse(Agent)` → `PostToolUse(Agent)` hook pair, correlated by
/// `tool_use_id`, cleared when the turn returns to idle — a foreground subagent
/// cannot outlive its turn.
///
/// A background subagent (`run_in_background: true`) outlives the launching
/// turn: its `PostToolUse` fires immediately at launch (the call returned, not
/// the subagent), and its real completion arrives much later as a
/// `<task-notification>` transcript line. So a background entry is NOT finished
/// by the immediate `PostToolUse` and is NOT swept at turn end; it is finished
/// only when the completion notification is folded (see
/// `Effect::SubagentCompleted`). The [`Self::background`] flag drives both
/// distinctions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningSubagent {
    /// The thread that launched the subagent, resolved (via
    /// `SessionStore::in_progress_turn_thread`) the same way the in-flight
    /// turn's thread is. A reconnecting client carries this so it can keep the
    /// launching thread's running indicator lit — and its unread badge
    /// suppressed — until the subagent finishes, which for a BACKGROUND
    /// subagent outlives the launching turn.
    pub thread_id: ThreadId,
    /// The `tool_use_id` of the `Agent`/`Task` call, the primary key that
    /// finishes it — the matching `PostToolUse` for a foreground entry, or the
    /// completion `<task-notification>` carrying this same id for a background
    /// entry.
    pub tool_use_id: String,
    /// The background-task identifier the launching tool's `tool_result`
    /// reports for a BACKGROUND subagent. Learned via the `PostToolUse(Agent)`
    /// hook (which reads `agentId` from the result content) and used as a
    /// fallback correlation key when matching a `<task-notification>` whose
    /// `<tool-use-id>` element was stripped from the user-message body — the
    /// notification's `<task-id>` element still routes back here. `None` until
    /// that hook has run, and stays `None` for foreground subagents (their
    /// `PostToolUse` finishes the entry directly, so the fallback key is
    /// never needed).
    pub task_id: Option<String>,
    /// The subagent type from the tool input (e.g. `general-purpose`), if the
    /// call carried one.
    pub subagent_type: Option<String>,
    /// The short task description from the tool input, if the call carried one,
    /// for display next to the indicator.
    pub description: Option<String>,
    /// Whether the launch carried `run_in_background: true`. A background
    /// subagent survives the immediate `PostToolUse` and the turn-end sweep; a
    /// foreground one is finished on its `PostToolUse` and swept at turn end.
    pub background: bool,
}

/// One consistent snapshot of the runtime state the sends envelope reports:
/// the turn phase plus the pending permission dialog, the pending question, and
/// the set of running subagents, read in a single actor message so they can
/// never disagree within one response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionLiveState {
    pub turn: TurnState,
    /// The thread the in-flight turn is running on, when a turn is in flight.
    /// `None` while idle. Lets a reconnecting client re-seed its per-thread
    /// running indicator on the exact thread (main or a branch) without waiting
    /// for the next turn-lifecycle event.
    pub in_progress_thread: Option<ThreadId>,
    pub pending_permission: Option<PendingPermission>,
    pub pending_question: Option<PendingQuestion>,
    /// The subagents currently running in this session's turn, oldest first.
    pub running_subagents: Vec<RunningSubagent>,
}

/// The live, provisional preview of the in-flight turn's assistant message,
/// accumulated from the `MessageDisplay` hook's chunks.
///
/// Claude Code streams the visible assistant text in chunks (one display
/// segment each) before the transcript JSONL is flushed. Delta buffers them
/// here so the browser can show the reply forming at the conversation tail —
/// including an assistant's pre-tool preamble, which appears before a blocking
/// tool prompt blocks. It is never persisted: the chunks share one `message_id`
/// that does not match any transcript id, so this is reconciled per turn — it
/// is cleared when the turn ends (see [`SessionRuntime::apply_turn`]) and the
/// persisted message, ingested by the normal transcript sync, renders instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamingMessage {
    /// The hook's display-message id (not a transcript id). A chunk whose id
    /// differs from the current buffer's starts a fresh message.
    pub message_id: String,
    /// The in-flight turn's thread, so the browser only shows the preview on
    /// the thread it belongs to.
    pub thread_id: ThreadId,
    /// The chunks received so far, paired with their `index`. Kept sparse and
    /// joined in index order on read, so out-of-order delivery is tolerated.
    pub chunks: Vec<(u32, String)>,
    /// Whether the final chunk has arrived.
    pub final_: bool,
}

impl StreamingMessage {
    /// The accumulated text so far, chunks joined in `index` order.
    ///
    /// The server broadcasts deltas incrementally (the client accumulates), so
    /// the joined text is only read back by tests asserting accumulation.
    #[cfg(test)]
    pub fn text(&self) -> String {
        let mut ordered: Vec<&(u32, String)> = self.chunks.iter().collect();
        ordered.sort_by_key(|(index, _)| *index);
        ordered
            .into_iter()
            .map(|(_, chunk)| chunk.as_str())
            .collect()
    }
}

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

    /// Whether a pane is live: bound to the session, or spawned and awaiting
    /// its first `UserPromptSubmit`. Used to keep the single-session cold
    /// start idempotent. A terminal-less agent session also counts as live so
    /// the cold-start idempotence check does not spawn a second pane alongside
    /// an open Codex session.
    pub fn has_live_pane(&self) -> bool {
        self.open.is_some() || self.pending_spawn.is_some() || self.open_agent.is_some()
    }

    /// The open **pane** handle, if the session is currently open on a
    /// pane-backed provider. Always `None` for a terminal-less agent session,
    /// which is exactly what makes the PTY bridge refuse to attach to a Codex
    /// session (it has no pane).
    pub fn handle(&self) -> Option<&OpenHandle> {
        self.open.as_ref()
    }

    /// Whether the session is currently open — a live, bound pane (Claude) or a
    /// live terminal-less agent session (Codex).
    pub fn is_open(&self) -> bool {
        self.open.is_some() || self.open_agent.is_some()
    }

    /// Mark the session open on a terminal-less agent (Codex), holding its live
    /// adapter and handle. The pane-backed [`Self::open`] slot is left `None`.
    pub fn bind_agent(&mut self, agent: OpenAgentSession) {
        self.open_agent = Some(agent);
    }

    /// Remove the terminal-less agent session (closing it), returning it so the
    /// caller can drive the adapter's `close`.
    pub fn remove_open_agent(&mut self) -> Option<OpenAgentSession> {
        self.open_agent.take()
    }

    /// Bind a pane to the session (the session is now open).
    pub fn bind(&mut self, handle: OpenHandle) {
        self.open = Some(handle);
    }

    /// Remove the bound pane (closing the session), returning its handle.
    pub fn remove_open(&mut self) -> Option<OpenHandle> {
        self.open.take()
    }

    /// Record a freshly-spawned pane awaiting its first `UserPromptSubmit`.
    pub fn push_pending(&mut self, spawn: PendingSpawn) {
        debug_assert!(
            self.pending_spawn.is_none(),
            "a session id is minted per spawn, so at most one spawn is ever pending"
        );
        self.pending_spawn = Some(spawn);
    }

    /// Idempotently bind the pending spawn, returning whether the bind was
    /// performed by *this* call.
    ///
    /// This is the single, order-independent binding step shared by the two
    /// signals that can register a fresh spawn — `SessionStart(source=startup)`
    /// and the first `UserPromptSubmit`. Whichever arrives first moves the
    /// [`PendingSpawn`] into the bound pane and returns `true`; whichever
    /// arrives second finds nothing pending, so it returns `false` and the
    /// already-bound session is left untouched (a no-op, including for
    /// sessions that never had a pending spawn at all).
    pub fn bind_pending_spawn(&mut self) -> bool {
        let Some(spawn) = self.pending_spawn.take() else {
            return false;
        };
        self.bind(OpenHandle {
            token: spawn.token,
            pane: spawn.pane,
        });
        true
    }

    /// Take the still-pending (unbound) spawn, removing it.
    ///
    /// Used by the failure path (the `SessionEnd` hook): a launch that ended
    /// while still unbound is removed here so its tmux pane can be cleaned up
    /// and a `SpawnFailed` emitted. Returns `None` when nothing is pending —
    /// which includes the normal case where the session is already bound, so
    /// the caller can tell a failed launch apart from a normal end.
    pub fn take_unbound_pending(&mut self) -> Option<PendingSpawn> {
        self.pending_spawn.take()
    }

    /// Drop the pending spawn if it carries this token.
    ///
    /// Used to roll back a spawn whose launch failed, so a half-spawned pane
    /// is not left pending where a later hook could mis-bind to it.
    pub fn remove_pending_for_token(&mut self, token: &PaneToken) {
        if self
            .pending_spawn
            .as_ref()
            .is_some_and(|p| &p.token == token)
        {
            self.pending_spawn = None;
        }
    }

    /// Take the unbound spawn if its deadline has passed as of `now`, leaving
    /// a still-fresh or already-bound one in place.
    ///
    /// `now` is supplied by the caller rather than read here so the watchdog is
    /// deterministic under test, and `deadline` comes from the caller's
    /// [`LaunchConfig`] ([`PENDING_SPAWN_DEADLINE`] in production; tests may
    /// shrink it). Once a spawn binds it moves out of the pending slot, so a
    /// bound session is never reaped.
    ///
    /// [`LaunchConfig`]: crate::launch_config::LaunchConfig
    pub fn take_stale_pending(&mut self, now: Instant, deadline: Duration) -> Option<PendingSpawn> {
        if self
            .pending_spawn
            .as_ref()
            .is_some_and(|p| now.duration_since(p.created_at) >= deadline)
        {
            return self.pending_spawn.take();
        }
        None
    }

    /// Record a resumed session whose first prompt is held until its
    /// `SessionStart(source=resume)` arrives. The pane is already bound; this
    /// only tracks the not-ready state and the held keystroke for the watchdog.
    pub fn start_resuming(&mut self, resuming: ResumingSession) {
        self.resuming = Some(resuming);
    }

    /// Attach a held first prompt to the resuming session, returning `true`
    /// when it was recorded. Returns `false` when the session is not resuming
    /// (already ready, or never resumed), so the caller dispatches the
    /// keystroke now instead of holding it.
    pub fn hold_first_prompt(&mut self, prompt: String) -> bool {
        match self.resuming.as_mut() {
            Some(resuming) => {
                resuming.held_prompt = Some(prompt);
                true
            }
            None => false,
        }
    }

    /// Whether a not-yet-dispatched resume entry exists — the session is
    /// inside its resume-readiness window, from `open_session` until the
    /// settle tick dispatches (or the resume fails). While this is true the
    /// pane is bound but `claude` may not yet accept input, so no keystroke
    /// may be typed: new first prompts are held via
    /// [`Self::hold_first_prompt`], and the queued-send dispatch defers until
    /// resume settle.
    pub fn is_resuming(&self) -> bool {
        self.resuming.is_some()
    }

    /// Take the resuming entry out, if present.
    ///
    /// This is the *removing* variant, used by the failure paths — `SessionEnd`
    /// for a resume that ended before becoming ready — which need to both
    /// detect the not-yet-ready resume and tear it down. It is **not** used by
    /// the `SessionStart(source=resume)` readiness hook: that hook must keep
    /// the entry (so the held prompt can be dispatched later, off the tick)
    /// and instead uses [`Self::mark_resume_ready_at`]. Returns `None` when
    /// the session is not resuming, making the failure hook an idempotent,
    /// safe no-op there.
    pub fn take_resuming(&mut self) -> Option<ResumingSession> {
        self.resuming.take()
    }

    /// Mark the resuming session ready *in place* by stamping its `ready_at`,
    /// keeping the entry. Returns whether the session was resuming.
    ///
    /// This is the `SessionStart(source=resume)` readiness path. It deliberately
    /// does **not** dispatch the held prompt: that hook blocks `claude` until its
    /// handler returns, so a keystroke typed here would be lost to a still-blocked
    /// TUI. Marking ready returns immediately (unblocking `claude`); the held
    /// keystroke is dispatched later by [`Self::take_ready_for_dispatch`] on a
    /// background tick, after the hook has returned and `claude` is input-ready.
    ///
    /// Idempotent: a repeated readiness hook just re-stamps `ready_at`. Returns
    /// `false` when the session is not resuming (already dispatched, never
    /// resumed, or a fresh spawn), so the hook is a safe no-op there.
    pub fn mark_resume_ready_at(&mut self, now: Instant) -> bool {
        match self.resuming.as_mut() {
            Some(resuming) => {
                resuming.ready_at = Some(now);
                true
            }
            None => false,
        }
    }

    /// Remove and return the resuming entry if it has been marked ready long
    /// enough to dispatch its held prompt as of `now` — i.e. `ready_at` is
    /// `Some(t)` with `now - t >= RESUME_DISPATCH_SETTLE`.
    ///
    /// Run on the background tick (outside any hook handler), this is the second
    /// stage of the two-stage readiness gate: [`Self::mark_resume_ready_at`]
    /// stamps `ready_at` inside the (blocking) `SessionStart` hook, and this
    /// takes the now-input-ready resume so the caller dispatches its held
    /// keystroke via the normal `send_line` path. A ready resume whose settle
    /// has not yet elapsed is left in place for a later tick; a not-yet-ready
    /// resume (`ready_at == None`) is never returned here — it is the
    /// watchdog's concern.
    ///
    /// `now` is injected for deterministic tests, mirroring the watchdog drains.
    pub fn take_ready_for_dispatch(&mut self, now: Instant) -> Option<ResumingSession> {
        let settled = self.resuming.as_ref().is_some_and(|r| match r.ready_at {
            Some(t) => now.duration_since(t) >= RESUME_DISPATCH_SETTLE,
            None => false,
        });
        if settled {
            return self.resuming.take();
        }
        None
    }

    /// Remove and return the resuming entry if it *never became ready* before
    /// its readiness deadline passed as of `now`, also dropping the bound pane
    /// (it is being torn down). Mirrors [`Self::take_stale_pending`] for
    /// resumes.
    ///
    /// A resume is stale only when it is still not-ready (`ready_at == None`)
    /// AND `now - created_at` has reached the deadline. A resume that became
    /// ready (`ready_at == Some`) is **not** reaped even past the deadline: it
    /// is merely pending dispatch on the tick, so reaping it would kill a
    /// healthy resume that is about to type its first prompt. Such a ready
    /// resume leaves via [`Self::take_ready_for_dispatch`], not here.
    pub fn take_stale_resuming(
        &mut self,
        now: Instant,
        deadline: Duration,
    ) -> Option<ResumingSession> {
        let stale = self
            .resuming
            .as_ref()
            .is_some_and(|r| r.ready_at.is_none() && now.duration_since(r.created_at) >= deadline);
        if stale {
            // The pane is being killed by the caller, so drop it from the
            // bound slot too — otherwise a failed resume would linger as
            // "open" with a dead pane.
            self.open = None;
            return self.resuming.take();
        }
        None
    }

    /// The session's current turn state.
    pub fn turn(&self) -> TurnState {
        self.turn
    }

    /// Snapshot the queryable live state (turn phase + pending permission +
    /// pending question + running subagents) in one read, for the sends
    /// envelope.
    /// The `in_progress_thread` is left `None` here and filled in by the actor
    /// handler, which has the store needed to resolve the in-flight turn's
    /// thread; this snapshot owns only the runtime fields it already holds.
    pub fn live_state(&self) -> SessionLiveState {
        SessionLiveState {
            turn: self.turn,
            in_progress_thread: None,
            pending_permission: self.pending_permission.clone(),
            pending_question: self.pending_question.clone(),
            running_subagents: self.running_subagents.clone(),
        }
    }

    /// Apply one input to the turn state machine, returning the full
    /// transition (the caller executes the orphan disposition and logs
    /// anomalies). The transition table lives in the `turn` module.
    ///
    /// A transition back to [`TurnState::Idle`] (stop, interrupt, close) also
    /// drops any pending permission dialog and pending question: both blocked
    /// that turn, so the turn ending — however it ended — makes them moot. This
    /// is the same lifecycle the browser notices have.
    pub fn apply_turn(&mut self, input: TurnInput) -> Transition {
        let result = transition(self.turn, input);
        self.turn = result.next;
        if result.next == TurnState::Idle {
            self.pending_permission = None;
            self.pending_question = None;
            // A FOREGROUND subagent cannot outlive the turn that spawned it:
            // once the turn ends (stop, interrupt, close) any still-running
            // foreground entry is moot, so drop it. This also covers the case
            // where a foreground `PostToolUse(Agent)` was somehow missed — the
            // turn end clears it rather than leaving a stuck indicator. A
            // BACKGROUND subagent (`run_in_background: true`) deliberately
            // outlives the launching turn: it keeps running after the turn
            // returns to idle, so it is kept here and removed only when its
            // completion `<task-notification>` is folded.
            self.running_subagents.retain(|s| s.background);
            // The provisional live preview belongs to the turn that just ended;
            // the persisted assistant message (ingested by the transcript sync)
            // now renders instead, so drop the preview to avoid a duplicate.
            self.streaming_message = None;
        }
        result
    }

    /// Remove and return ALL running-subagent entries, regardless of kind.
    ///
    /// This is the **process-gone sweep**, used by the two graceful signals
    /// that the session's `claude` process is confirmed gone —
    /// `on_session_end`'s normal-end path and `close_session`. Once the process
    /// is gone no more of this session's transcript is ingested, so a BACKGROUND
    /// entry's completion `<task-notification>` can never be folded and
    /// [`Self::finish_subagent`] can never fire for it: the indicator would
    /// otherwise stay lit forever. Draining hands every lingering entry back to
    /// the caller so it can emit a `SubagentFinished` per entry and clear the
    /// persisted launch row.
    ///
    /// How it differs from the other clears:
    /// - [`Self::finish_subagent`] removes ONE entry, driven by a single folded
    ///   completion notification — the normal, process-alive end.
    /// - [`Self::forget_turn`] also clears the whole set, but on session
    ///   DELETION and event-lessly (the persisted rows go by cascade). This
    ///   returns the drained entries precisely because the session still
    ///   exists, so the caller must emit events and drop persisted state itself.
    ///
    /// At both call sites the `TurnInput::Close` transition has already swept
    /// the foreground entries (see [`Self::apply_turn`]), so in practice this
    /// returns the surviving BACKGROUND entries. Draining the whole set anyway
    /// is deliberately kind-agnostic so nothing lingering can be missed.
    pub fn drain_running_subagents(&mut self) -> Vec<RunningSubagent> {
        std::mem::take(&mut self.running_subagents)
    }

    /// Drop the turn state without any orphan handling. Used when the session
    /// row itself is being deleted (its sends go with it by cascade).
    ///
    /// Unlike [`Self::apply_turn`], this clears the WHOLE running set including
    /// background subagents: the session is being deleted, so no later
    /// completion notification can arrive to finish a background entry — keeping
    /// one would pin a doomed actor alive forever.
    pub fn forget_turn(&mut self) {
        self.turn = TurnState::Idle;
        self.pending_permission = None;
        self.pending_question = None;
        self.running_subagents.clear();
        self.streaming_message = None;
    }

    /// Accumulate one `MessageDisplay` chunk into the live preview, returning
    /// the buffer's running text so the caller can broadcast the increment.
    ///
    /// A chunk whose `message_id` differs from the current buffer's starts a
    /// fresh preview (a new message began), as does the first chunk after a
    /// turn end cleared the buffer. Chunks are stored sparsely by `index` and
    /// joined in order on read, so out-of-order delivery is tolerated; a
    /// repeated `index` overwrites (the latest delivery wins).
    pub fn accumulate_streaming(
        &mut self,
        message_id: &str,
        thread_id: ThreadId,
        index: u32,
        final_: bool,
        delta: String,
    ) {
        let buffer = match self.streaming_message.as_mut() {
            Some(existing) if existing.message_id == message_id => existing,
            _ => {
                self.streaming_message = Some(StreamingMessage {
                    message_id: message_id.to_owned(),
                    thread_id,
                    chunks: Vec::new(),
                    final_: false,
                });
                self.streaming_message
                    .as_mut()
                    .expect("just inserted the streaming buffer")
            }
        };
        buffer.thread_id = thread_id;
        if let Some(slot) = buffer.chunks.iter_mut().find(|(i, _)| *i == index) {
            slot.1 = delta;
        } else {
            buffer.chunks.push((index, delta));
        }
        buffer.final_ = buffer.final_ || final_;
    }

    /// The current live preview, if a message is streaming.
    ///
    /// The preview is broadcast as it accumulates rather than read back in
    /// production, so this accessor exists for the streaming tests' assertions.
    #[cfg(test)]
    pub fn streaming_message(&self) -> Option<&StreamingMessage> {
        self.streaming_message.as_ref()
    }

    /// Register a oneshot waiter for a permission request the browser may
    /// decide, keyed by request-row id.
    pub fn insert_permission_waiter(
        &mut self,
        request_id: i64,
        sender: oneshot::Sender<PermissionDecision>,
    ) {
        self.permission_waiters.insert(request_id, sender);
    }

    /// Claim the waiter for a permission request, if it is still registered.
    /// Taking it is what makes two racing decisions unambiguous: the mailbox
    /// serializes them, and only the first finds the waiter.
    pub fn take_permission_waiter(
        &mut self,
        request_id: i64,
    ) -> Option<oneshot::Sender<PermissionDecision>> {
        self.permission_waiters.remove(&request_id)
    }

    /// Record the permission dialog now awaiting an answer (a new dialog
    /// replaces a stale one — `claude` shows one at a time).
    pub fn set_pending_permission(&mut self, pending: PendingPermission) {
        self.pending_permission = Some(pending);
    }

    /// Drop the pending dialog if `request_id` is the one it tracks. Keyed so
    /// a stale resolution can never wipe a newer dialog's state — the same
    /// guard the browser notice applies to `permission_resolved`.
    pub fn resolve_pending_permission(&mut self, request_id: i64) {
        if self
            .pending_permission
            .as_ref()
            .is_some_and(|p| p.request_id == request_id)
        {
            self.pending_permission = None;
        }
    }

    /// Record the `AskUserQuestion` now presenting its options in the TUI (a
    /// new question replaces a stale one — `claude` shows one at a time).
    pub fn set_pending_question(&mut self, pending: PendingQuestion) {
        self.pending_question = Some(pending);
    }

    /// The `AskUserQuestion` currently presenting its options in the TUI, if
    /// any. Read by the answer path to correlate an incoming answer by
    /// `request_id` and parse its question shapes for the key generator.
    pub fn pending_question(&self) -> Option<&PendingQuestion> {
        self.pending_question.as_ref()
    }

    /// Drop the pending question if `request_id` is the one it tracks. Keyed so
    /// a stale resolution can never wipe a newer question's state — the same
    /// guard [`Self::resolve_pending_permission`] applies.
    pub fn resolve_pending_question(&mut self, request_id: i64) {
        if self
            .pending_question
            .as_ref()
            .is_some_and(|q| q.request_id == request_id)
        {
            self.pending_question = None;
        }
    }

    /// Record a subagent (`Agent`/`Task` tool call) as started, returning
    /// whether it was newly added.
    ///
    /// Keyed by `tool_use_id`: a duplicate `PreToolUse` for an already-tracked
    /// id is a no-op (returns `false`), so a retried hook delivery cannot list
    /// the same subagent twice. New entries are appended so the set stays in
    /// start order for display.
    pub fn start_subagent(&mut self, subagent: RunningSubagent) -> bool {
        if self
            .running_subagents
            .iter()
            .any(|s| s.tool_use_id == subagent.tool_use_id)
        {
            return false;
        }
        self.running_subagents.push(subagent);
        true
    }

    /// Drop the FOREGROUND running subagent with this `tool_use_id`, returning
    /// whether one was actually removed.
    ///
    /// This is the `PostToolUse(Agent)` path. It only removes a foreground
    /// entry: a background subagent's `PostToolUse` fires immediately at launch
    /// (the call returned, not the subagent), so it must NOT finish it — the
    /// completion `<task-notification>` does, via [`Self::finish_subagent`].
    ///
    /// Keyed so a `PostToolUse` for an unknown id, one already cleared at turn
    /// end, or a background id (still running) is a harmless no-op (returns
    /// `false`) rather than emitting a spurious "finished".
    pub fn finish_foreground_subagent(&mut self, tool_use_id: &str) -> bool {
        let before = self.running_subagents.len();
        self.running_subagents
            .retain(|s| s.tool_use_id != tool_use_id || s.background);
        self.running_subagents.len() != before
    }

    /// Drop the running subagent with this `tool_use_id` regardless of kind,
    /// returning whether one was actually removed.
    ///
    /// This is the background-completion path: when a completion
    /// `<task-notification>` is folded (`Effect::SubagentCompleted`), the
    /// background entry it correlates to by `tool_use_id` is removed here.
    ///
    /// Keyed and kind-agnostic so it tolerates an unknown id: a background
    /// `Bash` (`run_in_background: true`) also produces `SubagentCompleted`, but
    /// Delta never STARTS an indicator for `Bash`, so its id is untracked and
    /// this is a harmless no-op (returns `false`).
    pub fn finish_subagent(&mut self, tool_use_id: &str) -> bool {
        let before = self.running_subagents.len();
        self.running_subagents
            .retain(|s| s.tool_use_id != tool_use_id);
        self.running_subagents.len() != before
    }

    /// Attach a learned `task_id` to the running subagent with this
    /// `tool_use_id`, returning `true` when the entry's `task_id` actually
    /// changed (so the caller knows to persist the upgrade through the store).
    /// Upgrading an unknown id (or an entry already carrying a matching
    /// `task_id`) returns `false` — no row was changed, so nothing downstream
    /// needs to fire.
    ///
    /// This is the BACKGROUND subagent's `PostToolUse(Agent)` path: the hook
    /// reads `agentId` from the launching tool's `tool_result` and records it
    /// here so a subsequent `<task-notification>` whose `<tool-use-id>` element
    /// was stripped can still be matched by its `<task-id>` element.
    pub fn upgrade_subagent_task_id(&mut self, tool_use_id: &str, task_id: &str) -> bool {
        let Some(entry) = self
            .running_subagents
            .iter_mut()
            .find(|s| s.tool_use_id == tool_use_id)
        else {
            return false;
        };
        if entry.task_id.as_deref() == Some(task_id) {
            return false;
        }
        entry.task_id = Some(task_id.to_owned());
        true
    }

    /// The `task_id` the runtime knows for this `tool_use_id`, if any.
    ///
    /// Read by the sync path right after [`Effect::SubagentLaunched`] persists
    /// the launch row: a background subagent's immediate `PostToolUse(Agent)`
    /// usually fires before the launch line is folded, so the hook recorded
    /// the `agentId` on the runtime entry but could not yet persist it on the
    /// launch row (which did not exist). The sync flushes that pending upgrade
    /// here so the persisted row carries the fallback correlation key for the
    /// eventual `<task-notification>`.
    ///
    /// [`Effect::SubagentLaunched`]: delta_attribution::Effect::SubagentLaunched
    pub fn pending_subagent_task_id(&self, tool_use_id: &str) -> Option<&str> {
        self.running_subagents
            .iter()
            .find(|s| s.tool_use_id == tool_use_id)
            .and_then(|s| s.task_id.as_deref())
    }

    /// Record the `agentId` a `PostToolUse(Agent)` reported, so the next
    /// transcript sync can fold it into the running-subagent entry once that
    /// entry exists. Entry-or-insert: a retried hook delivery for the same
    /// `tool_use_id` does not overwrite the first observed value.
    ///
    /// See [`Self::pending_post_tool_use_agent_ids`] for the race this buffer
    /// covers.
    ///
    /// [`Self::pending_post_tool_use_agent_ids`]: SessionRuntime::pending_post_tool_use_agent_ids
    pub(in crate::interactor) fn record_pending_post_tool_use_agent_id(
        &mut self,
        tool_use_id: &str,
        agent_id: &str,
    ) {
        self.pending_post_tool_use_agent_ids
            .entry(tool_use_id.to_owned())
            .or_insert_with(|| agent_id.to_owned());
    }

    /// Take the buffered `agentId` for this `tool_use_id`, if any. Drained by
    /// the `Effect::SubagentIndicatorStarted` arm of `sync_transcript` once it
    /// creates the in-memory running entry — after which the value, if present,
    /// is applied to the entry and persisted on the launch row.
    pub(in crate::interactor) fn drain_pending_post_tool_use_agent_id(
        &mut self,
        tool_use_id: &str,
    ) -> Option<String> {
        self.pending_post_tool_use_agent_ids.remove(tool_use_id)
    }

    /// Try to claim a window for an auto-compact re-dispatch as of `now`,
    /// returning `true` when the caller should proceed and `false` when a
    /// recent re-dispatch already covered the same compact event.
    ///
    /// On `true` the stamp is updated to `now`. The debounce window is
    /// [`AUTO_COMPACT_REDISPATCH_DEBOUNCE`]; see the field docstring on
    /// [`Self::last_auto_compact_redispatch_at`] for why both the hook path
    /// and the ingestion-effect path key on the same stamp.
    pub(in crate::interactor) fn try_claim_auto_compact_redispatch(
        &mut self,
        now: Instant,
    ) -> bool {
        let stale = self
            .last_auto_compact_redispatch_at
            .map(|t| now.duration_since(t) >= AUTO_COMPACT_REDISPATCH_DEBOUNCE)
            .unwrap_or(true);
        if stale {
            self.last_auto_compact_redispatch_at = Some(now);
        }
        stale
    }

    /// The pending spawn, for the test seams that read launch state back.
    #[cfg(test)]
    pub(crate) fn pending_spawn(&self) -> Option<&PendingSpawn> {
        self.pending_spawn.as_ref()
    }

    /// Whether a not-yet-dispatched resume entry exists, for the test seams.
    #[cfg(test)]
    pub(crate) fn resuming(&self) -> Option<&ResumingSession> {
        self.resuming.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn subagent(tool_use_id: &str, background: bool) -> RunningSubagent {
        RunningSubagent {
            thread_id: ThreadId(1),
            tool_use_id: tool_use_id.to_owned(),
            task_id: None,
            subagent_type: None,
            description: None,
            background,
        }
    }

    #[test]
    fn drain_running_subagents_returns_every_entry_and_empties_the_set() {
        let mut runtime = SessionRuntime::default();
        // A foreground and a background entry, so the drain is proven
        // kind-agnostic (unlike the turn-end sweep, which keeps background).
        runtime.start_subagent(subagent("toolu_fg", false));
        runtime.start_subagent(subagent("toolu_bg", true));

        let drained = runtime.drain_running_subagents();

        assert_eq!(
            drained
                .iter()
                .map(|s| s.tool_use_id.clone())
                .collect::<Vec<_>>(),
            vec!["toolu_fg".to_owned(), "toolu_bg".to_owned()],
            "drain returns all entries in start order, regardless of kind"
        );
        assert!(
            runtime.live_state().running_subagents.is_empty(),
            "drain leaves the running set empty"
        );
    }

    #[test]
    fn drain_running_subagents_is_empty_when_none_are_running() {
        let mut runtime = SessionRuntime::default();

        assert!(
            runtime.drain_running_subagents().is_empty(),
            "draining an empty set yields nothing"
        );
        assert!(runtime.live_state().running_subagents.is_empty());
    }
}
