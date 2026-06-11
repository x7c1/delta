//! The in-memory registry of live (open) Claude Code panes.
//!
//! Open/closed is **process-runtime** state, never persisted: after a restart
//! the registry is empty, so every session that survives in the store is
//! considered "closed" until it is resumed. The registry maps each open session
//! to the tmux pane driving it, and tracks freshly-spawned panes whose Claude
//! `session_id` was pinned by Delta at spawn time (via `claude --session-id`)
//! and which the first `UserPromptSubmit` hook then binds into the live map.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use delta_model::SessionId;

use crate::pane_token::PaneToken;

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

/// The result of an idempotent [`OpenSessions::bind_pending_spawn`] that
/// actually performed a bind, carrying the just-bound spawn's deferred first
/// prompt so the caller can write its `pending_send` row now that the session
/// id is known.
#[derive(Debug, Clone)]
pub struct BindOutcome {
    /// The deferred first send held on the spawn, if it was a composer-initiated
    /// New; `None` for a prompt-less plain spawn.
    pub first_prompt: Option<String>,
}

/// A live, bound session: its Claude `session_id` is known and it is mapped to
/// the tmux pane driving it.
#[derive(Debug, Clone)]
pub struct OpenHandle {
    /// The Delta-minted tmux session name.
    pub token: PaneToken,
    /// The pane keystrokes are sent to and the PTY attaches to (`<token>:0.0`).
    pub pane: String,
    /// The working directory the session runs in.
    pub workdir: String,
}

/// A freshly-spawned pane awaiting its first `UserPromptSubmit`.
///
/// Delta pins the conversation's `session_id` at spawn time by passing a
/// freshly-minted UUID to `claude --session-id <uuid>`, so the first
/// `UserPromptSubmit` hook reports exactly that id. The spawn is correlated to
/// its session by matching the hook's `session_id` against [`Self::session_id`]
/// — independent of the working directory, so two spawns may share a `cwd`
/// without mis-correlating. If the spawn was initiated by a composer send,
/// [`Self::first_prompt`] carries the text to enqueue once the session binds.
#[derive(Debug, Clone)]
pub struct PendingSpawn {
    /// The Delta-minted tmux session name.
    pub token: PaneToken,
    /// The pane keystrokes are sent to (`<token>:0.0`).
    pub pane: String,
    /// The Delta-minted Claude `session_id` pinned via `--session-id`; the
    /// binding key the first `UserPromptSubmit` hook is matched against.
    pub session_id: SessionId,
    /// The working directory this spawn runs in. Populates the [`OpenHandle`] at
    /// bind time and is kept as informational data; it is no longer the match
    /// key (correlation is by [`Self::session_id`]).
    pub workdir: String,
    /// The deferred first send, if this spawn was initiated by a composer send.
    ///
    /// The `pending_send` row cannot be written before the spawn binds (it
    /// references `session(id)`, which does not exist yet), so the text is held
    /// here and enqueued once the binding supplies the session id.
    pub first_prompt: Option<String>,
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
/// is held until the resume's `SessionStart` (`source=resume`) hook arrives.
///
/// Unlike a fresh spawn, a resume's `session_id` is known up front, so the pane
/// binds immediately in [`OpenSessions::bound`]. But `claude --resume` needs a
/// couple of seconds to replay the transcript and make its TUI input ready, far
/// longer than any fixed settle could safely cover. So Delta does not type the
/// first prompt at resume time; it records the resume here and dispatches the
/// held keystroke only once `SessionStart(source=resume)` confirms readiness.
/// The `pending_send` row for that first prompt is written normally (its thread,
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
}

/// The in-memory map of live panes.
///
/// Held behind a mutex by the interactor. Rebuilt empty on every boot.
#[derive(Debug, Default)]
pub struct OpenSessions {
    /// Sessions whose Claude `session_id` is known, keyed by it.
    bound: HashMap<SessionId, OpenHandle>,
    /// Spawned panes awaiting their first `UserPromptSubmit` to learn the id.
    pending: Vec<PendingSpawn>,
    /// Resumed sessions awaiting their `SessionStart(source=resume)` before the
    /// held first prompt is dispatched, keyed by the (already-known) session id.
    ///
    /// A session is in this map exactly while it is resumed-but-not-ready: it is
    /// inserted by `open_session`, removed when `SessionStart(source=resume)`
    /// marks it ready (the held prompt is dispatched then), and drained by the
    /// watchdog if its readiness deadline passes. Membership here is the "not
    /// ready yet, hold sends" flag; absence means the session is ready and sends
    /// dispatch immediately.
    resuming: HashMap<SessionId, ResumingSession>,
}

impl OpenSessions {
    /// Whether any pane is live: bound to a session, or spawned and awaiting its
    /// first `UserPromptSubmit`. Used to keep the single-session cold start
    /// idempotent.
    pub fn has_any_live(&self) -> bool {
        !self.bound.is_empty() || !self.pending.is_empty()
    }

    /// The open handle for a session, if it is currently open.
    pub fn handle(&self, id: &SessionId) -> Option<&OpenHandle> {
        self.bound.get(id)
    }

    /// Whether a specific session is currently open (has a live, bound pane).
    pub fn is_open(&self, id: &SessionId) -> bool {
        self.bound.contains_key(id)
    }

    /// Bind a freshly-spawned pane to a now-known session id.
    pub fn bind(&mut self, id: SessionId, handle: OpenHandle) {
        self.bound.insert(id, handle);
    }

    /// Idempotently bind the pending spawn matching `id`, returning its deferred
    /// first prompt when it was bound by *this* call.
    ///
    /// This is the single, order-independent binding step shared by the two
    /// signals that can register a fresh spawn — `SessionStart(source=startup)`
    /// and the first `UserPromptSubmit`. Whichever arrives first moves the
    /// matching [`PendingSpawn`] `pending → bound[id]` and yields its
    /// `first_prompt` (so the caller can write the deferred `pending_send`);
    /// whichever arrives second finds no pending spawn for `id`, so it returns
    /// `None` and the already-bound session is left untouched. The boolean
    /// distinguishes the two outcomes for the caller:
    ///
    /// - `Some(BindOutcome { first_prompt, .. })` — this call performed the bind.
    /// - `None` — already bound by a prior call (or no such pending spawn at
    ///   all); a no-op.
    pub fn bind_pending_spawn(&mut self, id: &SessionId) -> Option<BindOutcome> {
        let spawn = self.take_pending_for_session(id)?;
        let first_prompt = spawn.first_prompt;
        self.bind(
            id.clone(),
            OpenHandle {
                token: spawn.token,
                pane: spawn.pane,
                workdir: spawn.workdir,
            },
        );
        Some(BindOutcome { first_prompt })
    }

    /// Record a resumed session whose first prompt is held until its
    /// `SessionStart(source=resume)` arrives. The pane is already bound; this
    /// only tracks the not-ready state and the held keystroke for the watchdog.
    pub fn start_resuming(&mut self, id: SessionId, resuming: ResumingSession) {
        self.resuming.insert(id, resuming);
    }

    /// Whether a session is resumed but not yet ready (its first prompt is held).
    /// `false` for a fresh-spawned or already-ready session, so sends to it
    /// dispatch immediately.
    pub fn is_resuming(&self, id: &SessionId) -> bool {
        self.resuming.contains_key(id)
    }

    /// Attach a held first prompt to a resuming session, returning `true` when it
    /// was recorded. Returns `false` when the session is not resuming (already
    /// ready, or never resumed), so the caller dispatches the keystroke now
    /// instead of holding it.
    pub fn hold_first_prompt(&mut self, id: &SessionId, prompt: String) -> bool {
        match self.resuming.get_mut(id) {
            Some(resuming) => {
                resuming.held_prompt = Some(prompt);
                true
            }
            None => false,
        }
    }

    /// Mark a resuming session ready: remove it from the resuming map and return
    /// its held first prompt (if any) so the caller can dispatch it now.
    ///
    /// Returns `None` when the id is not resuming — the readiness hook for a
    /// session that already became ready, was never resumed, or is a fresh spawn
    /// — making `SessionStart(source=resume)` an idempotent, safe no-op there.
    pub fn mark_resume_ready(&mut self, id: &SessionId) -> Option<ResumingSession> {
        self.resuming.remove(id)
    }

    /// The session ids currently resuming-but-not-ready, in arbitrary order.
    ///
    /// Test-only seam: lets a test confirm a resume is being held (or has been
    /// released) without reaching into the private map.
    #[cfg(test)]
    pub(crate) fn resuming_session_ids(&self) -> Vec<SessionId> {
        self.resuming.keys().cloned().collect()
    }

    /// Remove and return every resuming session whose readiness deadline has
    /// passed as of `now`, also dropping each from the bound map (its pane is
    /// being torn down). Mirrors [`Self::drain_stale_pending`] for resumes.
    ///
    /// `now` is injected for deterministic tests. A resume is stale when
    /// `now - created_at` has reached [`RESUME_READY_DEADLINE`]. A resume that
    /// became ready in time was already removed from `resuming` by
    /// [`Self::mark_resume_ready`], so it is never drained here.
    pub fn drain_stale_resuming(&mut self, now: Instant) -> Vec<(SessionId, ResumingSession)> {
        let stale_ids: Vec<SessionId> = self
            .resuming
            .iter()
            .filter(|(_, r)| now.duration_since(r.created_at) >= RESUME_READY_DEADLINE)
            .map(|(id, _)| id.clone())
            .collect();
        stale_ids
            .into_iter()
            .map(|id| {
                let resuming = self.resuming.remove(&id).expect("just collected");
                // The pane is being killed by the caller, so drop it from the
                // bound map too — otherwise a failed resume would linger as
                // "open" with a dead pane.
                self.bound.remove(&id);
                (id, resuming)
            })
            .collect()
    }

    /// Record a freshly-spawned pane awaiting its first `UserPromptSubmit`.
    pub fn push_pending(&mut self, spawn: PendingSpawn) {
        self.pending.push(spawn);
    }

    /// Take the pending spawn whose Delta-minted session id equals `id`.
    ///
    /// Delta pins each fresh spawn's `session_id` up front via
    /// `claude --session-id`, so the first `UserPromptSubmit` hook reports
    /// exactly that id and this matches at most one spawn — independent of the
    /// working directory, so spawns sharing a `cwd` still correlate correctly.
    pub fn take_pending_for_session(&mut self, id: &SessionId) -> Option<PendingSpawn> {
        let idx = self.pending.iter().position(|p| &p.session_id == id)?;
        Some(self.pending.remove(idx))
    }

    /// The Delta-minted session ids of all currently-pending spawns, in order.
    ///
    /// Test-only seam: the production minted id is a random UUID a test cannot
    /// predict, so binding tests read it back here to fire the matching hook.
    #[cfg(test)]
    pub(crate) fn pending_session_ids(&self) -> Vec<SessionId> {
        self.pending.iter().map(|p| p.session_id.clone()).collect()
    }

    /// Drop the pending spawn with this token, if present.
    ///
    /// Used to roll back a spawn whose post-registration dispatch failed, so a
    /// half-spawned pane is not left in `pending` where a later, unrelated
    /// `UserPromptSubmit` could mis-bind to it.
    pub fn remove_pending_for_token(&mut self, token: &PaneToken) {
        self.pending.retain(|p| &p.token != token);
    }

    /// Take the still-pending (unbound) spawn whose Delta-minted session id
    /// equals `id`, removing it from the registry.
    ///
    /// Mirrors [`Self::take_pending_for_session`] but is used by the failure
    /// path (the `SessionEnd` hook): a launch that ended while still unbound is
    /// removed here so its tmux pane can be cleaned up and a `SpawnFailed`
    /// emitted. Returns `None` when no *pending* spawn carries this id — which
    /// includes the normal case where the id belongs to an already-bound
    /// session, so the caller can tell a failed launch apart from a normal end.
    pub fn take_unbound_pending_for_session(&mut self, id: &SessionId) -> Option<PendingSpawn> {
        self.take_pending_for_session(id)
    }

    /// Remove and return every unbound spawn whose deadline has passed as of
    /// `now`, leaving the still-fresh and the already-bound ones in place.
    ///
    /// `now` is supplied by the caller rather than read here so the watchdog is
    /// deterministic under test: a test seeds spawns with controlled
    /// `created_at` values and passes an explicit `now`, instead of depending on
    /// wall-clock elapsed time. A spawn is stale when `now - created_at` has
    /// reached [`PENDING_SPAWN_DEADLINE`]. Only spawns still in `pending` are
    /// considered: once a spawn binds it is moved out of `pending` into `bound`,
    /// so a bound session is never reaped.
    pub fn drain_stale_pending(&mut self, now: Instant) -> Vec<PendingSpawn> {
        let mut stale = Vec::new();
        let mut i = 0;
        while i < self.pending.len() {
            if now.duration_since(self.pending[i].created_at) >= PENDING_SPAWN_DEADLINE {
                stale.push(self.pending.remove(i));
            } else {
                i += 1;
            }
        }
        stale
    }

    /// Remove a session from the bound map (closing it), returning its handle.
    pub fn remove(&mut self, id: &SessionId) -> Option<OpenHandle> {
        self.bound.remove(id)
    }
}
