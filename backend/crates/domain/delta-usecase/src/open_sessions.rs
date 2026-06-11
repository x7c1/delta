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

/// The in-memory map of live panes.
///
/// Held behind a mutex by the interactor. Rebuilt empty on every boot.
#[derive(Debug, Default)]
pub struct OpenSessions {
    /// Sessions whose Claude `session_id` is known, keyed by it.
    bound: HashMap<SessionId, OpenHandle>,
    /// Spawned panes awaiting their first `UserPromptSubmit` to learn the id.
    pending: Vec<PendingSpawn>,
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
