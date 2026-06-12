//! The runtime state one session actor owns.
//!
//! Everything here is **process-runtime** state, never persisted: after a
//! restart every actor starts from [`SessionRuntime::default`], so every
//! session that survives in the store is considered "closed" (and its turn
//! idle) until it is resumed. One value exists per live actor; absence of an
//! actor reads exactly like this default, which is what makes actor
//! retirement (see the `actor` module) safe.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use tokio::sync::oneshot;

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

/// A live, bound session: its Claude `session_id` is known and it is mapped to
/// the tmux pane driving it.
#[derive(Debug, Clone)]
pub struct OpenHandle {
    /// The Delta-minted tmux session name.
    pub token: PaneToken,
    /// The pane keystrokes are sent to and the PTY attaches to (`<token>:0.0`).
    pub pane: String,
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

/// One consistent snapshot of the runtime state the sends envelope reports:
/// the turn phase plus the pending permission dialog, read in a single actor
/// message so the two can never disagree within one response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionLiveState {
    pub turn: TurnState,
    pub pending_permission: Option<PendingPermission>,
}

/// All of one session's runtime state, owned exclusively by its actor.
///
/// The actor's mailbox is the only way in, so no lock guards any of this: the
/// pane binding, the spawn/resume launch state, the turn state machine, and
/// the pending permission waiters all mutate strictly in mailbox order.
#[derive(Debug, Default)]
pub struct SessionRuntime {
    /// The live pane once the session is bound (open). `None` means closed.
    open: Option<OpenHandle>,
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
}

impl SessionRuntime {
    /// Whether this runtime is indistinguishable from a freshly-built one.
    ///
    /// When true the actor may retire (see the `actor` module): a later
    /// message for the session spawns a new actor whose default state means
    /// exactly the same thing.
    pub fn is_empty(&self) -> bool {
        self.open.is_none()
            && self.pending_spawn.is_none()
            && self.resuming.is_none()
            && self.turn == TurnState::Idle
            && self.permission_waiters.is_empty()
            && self.pending_permission.is_none()
    }

    /// Whether a pane is live: bound to the session, or spawned and awaiting
    /// its first `UserPromptSubmit`. Used to keep the single-session cold
    /// start idempotent.
    pub fn has_live_pane(&self) -> bool {
        self.open.is_some() || self.pending_spawn.is_some()
    }

    /// The open handle, if the session is currently open.
    pub fn handle(&self) -> Option<&OpenHandle> {
        self.open.as_ref()
    }

    /// Whether the session is currently open (has a live, bound pane).
    pub fn is_open(&self) -> bool {
        self.open.is_some()
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
        if self.pending_spawn.as_ref().is_some_and(|p| &p.token == token) {
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

    /// Snapshot the queryable live state (turn phase + pending permission)
    /// in one read, for the sends envelope.
    pub fn live_state(&self) -> SessionLiveState {
        SessionLiveState {
            turn: self.turn,
            pending_permission: self.pending_permission.clone(),
        }
    }

    /// Apply one input to the turn state machine, returning the full
    /// transition (the caller executes the orphan disposition and logs
    /// anomalies). The transition table lives in the `turn` module.
    ///
    /// A transition back to [`TurnState::Idle`] (stop, interrupt, close) also
    /// drops any pending permission dialog: the dialog blocked that turn, so
    /// the turn ending — however it ended — means the question is moot. This
    /// is the same lifecycle the browser notice has.
    pub fn apply_turn(&mut self, input: TurnInput) -> Transition {
        let result = transition(self.turn, input);
        self.turn = result.next;
        if result.next == TurnState::Idle {
            self.pending_permission = None;
        }
        result
    }

    /// Drop the turn state without any orphan handling. Used when the session
    /// row itself is being deleted (its sends go with it by cascade).
    pub fn forget_turn(&mut self) {
        self.turn = TurnState::Idle;
        self.pending_permission = None;
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
