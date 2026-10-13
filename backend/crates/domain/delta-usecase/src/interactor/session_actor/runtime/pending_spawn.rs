//! A spawn awaiting its first `UserPromptSubmit`.

use std::time::Instant;

use crate::pane_token::PaneToken;

#[cfg(doc)]
use super::{SessionRuntime, PENDING_SPAWN_DEADLINE};

/// A spawn whose pane is being (or has been) created, awaiting its first
/// `UserPromptSubmit`.
///
/// Delta pins the conversation's `session_id` at spawn time by passing a
/// freshly-minted UUID to `claude --session-id <uuid>`, so the first
/// `UserPromptSubmit` hook reports exactly that id — which routes the hook to
/// this spawn's actor, whose pending entry this is. The session row (and any
/// first prompt's send row) is written eagerly at spawn time, so binding only
/// activates that row and maps the pane.
///
/// Recorded *just before* the pane is created, never after: the launch task
/// awaits the actor's `LaunchPrepared` reply before it calls `create_session`,
/// so this entry is always in place by the time the launched agent can fire a
/// hook against it.
#[derive(Debug, Clone)]
pub struct PendingSpawn {
    /// The Delta-minted tmux session name.
    pub token: PaneToken,
    /// The pane keystrokes are sent to (`<token>:0.0`).
    pub pane: String,
    /// When this spawn's bind deadline started running.
    ///
    /// A spawn is fire-and-forget: only the first `UserPromptSubmit` hook binds
    /// it, so a launch that crashes/hangs before that hook never times out on
    /// its own. The reaper compares `now - created_at` against
    /// [`PENDING_SPAWN_DEADLINE`] to detect and clean up such a stuck spawn.
    /// `Instant` is monotonic, so it measures elapsed wall time without being
    /// perturbed by system-clock changes.
    ///
    /// Normally the moment the spawn was recorded, but not for a pane a browser
    /// has been attached to: the last bridge leaving restarts the deadline from
    /// there (see [`SessionRuntime::restart_pending_deadline`]).
    pub created_at: Instant,
    /// Whether the tmux session behind [`Self::pane`] actually exists yet.
    ///
    /// The entry is recorded a beat *before* `create_session` runs — the hooks
    /// the launched agent fires bind this record, so it has to be in place
    /// first — which leaves a short window where the pane is named but not
    /// there. Nothing may attach in that window, so the launch flips this when
    /// it reports success, at the same point the browser is told the pane is up
    /// (`SessionEvent::SpawnPaneReady`). Binding does not depend on it: a hook
    /// arriving from inside `create_session` binds the spawn as it always did.
    pub pane_created: bool,
}
