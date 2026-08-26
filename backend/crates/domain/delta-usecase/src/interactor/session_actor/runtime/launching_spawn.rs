//! The accept→launch window: the state an actor holds between answering a
//! new-session send and its background launch preparation reporting back,
//! with the two transitions that open and close that window.

use std::time::Instant;

use crate::pane_token::PaneToken;

use super::{PlannedWorktree, SessionRuntime};

/// An accepted session whose launch preparation is still running.
///
/// `POST /api/sends { new_session: true }` answers as soon as the session is
/// *accepted*: the row exists (listed as `spawning`), the first send is
/// recorded, and the response carries real ids. Everything expensive — the
/// worktree build, the trust seed, writing the settings file, and launching
/// `claude` in a tmux pane — happens afterwards on a background task, which
/// reports back through this actor's own mailbox
/// ([`SessionInput::LaunchFinished`]). This entry is what the actor holds in
/// between: no pane exists yet, so nothing can bind, but the session is
/// emphatically not idle.
///
/// Its presence makes [`SessionRuntime::has_live_pane`] true (a cold start must
/// not spawn a second session alongside it), keeps
/// [`SessionRuntime::is_empty`] false (the actor must stay alive to receive
/// the launch's outcome), and makes
/// [`SessionRuntime::is_launching_or_pending`] true (a send arriving now is
/// refused with `session_spawning`, exactly as one arriving against a
/// [`PendingSpawn`] is). The watchdog drains deliberately do NOT see it: a
/// launch preparation has its own deadline on the task, and the bind deadline
/// only starts once a pane actually exists.
///
/// [`SessionInput::LaunchFinished`]: crate::interactor::session_actor::input::SessionInput::LaunchFinished
/// [`PendingSpawn`]: super::PendingSpawn
#[derive(Debug, Clone)]
pub struct LaunchingSpawn {
    /// The Delta-minted tmux session name the launch will create.
    pub token: PaneToken,
    /// The pane keystrokes will be sent to (`<token>:0.0`) once it exists.
    pub pane: String,
    /// The directory the agent will be launched in, as planned by the accept
    /// phase — the worktree path for a worktree spawn, the user-selected
    /// directory for a plain one, the per-token scratch dir otherwise. Also
    /// what the eager session row already stored as its `cwd`, which is why it
    /// is computed before the build rather than read back from it.
    pub workdir: String,
    /// The worktree still to build, when one was requested. `None` for a plain
    /// spawn, which has no git work left to do.
    pub worktree: Option<PlannedWorktree>,
    /// Whether Claude Code's workspace-trust dialog must be pre-accepted for
    /// [`Self::workdir`] before launching there.
    pub seed_trust: bool,
    /// The full argv the launch runs (`claude --settings … --session-id …`,
    /// the user's launch options, then any first prompt).
    pub command: Vec<String>,
    /// When the send was accepted — the instant the REST response went out.
    ///
    /// Not a watchdog deadline (the launch task owns its own timeout): it is
    /// the stamp that lets the launch's log line report how long the
    /// preparation actually took, which is the number this whole split exists
    /// to keep out of the request.
    pub accepted_at: Instant,
}

impl SessionRuntime {
    /// Record an accepted session whose launch preparation is now running in
    /// the background.
    pub fn start_launching(&mut self, launching: LaunchingSpawn) {
        debug_assert!(
            self.launching_spawn.is_none() && self.pending_spawn.is_none(),
            "a session id is minted per spawn, so at most one launch is ever in flight"
        );
        self.launching_spawn = Some(launching);
    }

    /// Take the in-flight launch back out if it carries this token — the
    /// launch task reporting its outcome.
    ///
    /// Keyed by token so a late report from a launch that was already rolled
    /// back (or, defensively, from some other launch) cannot consume an
    /// unrelated entry. `None` means there is nothing left to finish, and the
    /// caller treats the report as stale.
    pub fn take_launching_for_token(&mut self, token: &PaneToken) -> Option<LaunchingSpawn> {
        if self
            .launching_spawn
            .as_ref()
            .is_some_and(|l| &l.token == token)
        {
            return self.launching_spawn.take();
        }
        None
    }

    /// The in-flight launch, for the test seams that read launch state back.
    #[cfg(test)]
    pub(crate) fn launching_spawn(&self) -> Option<&LaunchingSpawn> {
        self.launching_spawn.as_ref()
    }
}
