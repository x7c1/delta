//! The bind-and-resume launch state: a fresh spawn awaiting its first bind
//! ([`PendingSpawn`]) and a resumed session holding its first prompt until
//! ready ([`ResumingSession`]), with the watchdog deadlines and drains. The
//! window *before* a spawn has a pane at all lives in the `launching_spawn`
//! module.

use std::time::{Duration, Instant};

use crate::pane_token::PaneToken;

use super::{OpenHandle, SessionRuntime};

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

impl SessionRuntime {
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

    /// Whether a fresh spawn has yet to bind — either its launch preparation is
    /// still running ([`LaunchingSpawn`]) or its pane is up and awaiting the
    /// first hook ([`PendingSpawn`]). The session row exists (and is listed as
    /// `spawning`) throughout, but no pane is mapped to it yet.
    ///
    /// Read by the enqueue path: a send arriving in this window must not take
    /// the `ensure_open()` → `open_session()` (`claude --resume <id>`) route,
    /// which would launch a second agent against a transcript the first launch
    /// has not written yet. Both sub-states answer the same way — the window
    /// starts the moment the first send is accepted, which is *before* the
    /// launch has been prepared at all. Non-destructive, unlike
    /// [`Self::take_unbound_pending`], so the spawn still binds normally.
    ///
    /// [`LaunchingSpawn`]: super::LaunchingSpawn
    pub fn is_launching_or_pending(&self) -> bool {
        self.launching_spawn.is_some() || self.pending_spawn.is_some()
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

    /// Take the unbound spawn if its deadline has passed as of `now`, leaving
    /// a still-fresh or already-bound one in place.
    ///
    /// `now` is supplied by the caller rather than read here so the watchdog is
    /// deterministic under test, and `deadline` comes from the caller's
    /// [`LaunchConfig`] ([`PENDING_SPAWN_DEADLINE`] in production; tests may
    /// shrink it). Once a spawn binds it moves out of the pending slot, so a
    /// bound session is never reaped.
    ///
    /// A session whose launch preparation is still running carries a
    /// [`LaunchingSpawn`], not a [`PendingSpawn`], so it is invisible here on
    /// purpose: it has no pane to kill, and its bind deadline only starts once
    /// the launch has actually produced one (see
    /// [`Self::start_launching`]). A slow `git fetch` therefore cannot eat the
    /// bind deadline.
    ///
    /// [`LaunchConfig`]: crate::launch_config::LaunchConfig
    /// [`LaunchingSpawn`]: super::LaunchingSpawn
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

    /// Drop the held first prompt, keeping the resuming entry itself, and
    /// report whether one was actually held.
    ///
    /// The counterpart to [`Self::hold_first_prompt`], for the case where the
    /// held send stops being this resume's to type: a prompt submitted inside
    /// the resume window cannot be the held send's (its keystrokes are still
    /// here, not in the pane), so the turn machine requeues that send — and
    /// the queue is now the single owner of the message. Leaving the text here
    /// too would deliver it twice: the settle would type this copy AND the
    /// next idle flush would dispatch the `queued` row. Dropping it makes the
    /// settle take its "no held first prompt; flushing any queued send"
    /// branch, so the row is typed exactly once, on the normal queued path.
    ///
    /// A no-op (returning `false`) when the session is not resuming or was
    /// holding nothing, so the caller may fire it unconditionally.
    pub fn drop_held_prompt(&mut self) -> bool {
        match self.resuming.as_mut() {
            Some(resuming) => resuming.held_prompt.take().is_some(),
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
