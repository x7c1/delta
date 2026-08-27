//! The actor's half of the launch's *last* step: recording the spawn the pane
//! about to be created will fire its hooks against.

use std::time::Instant;

use crate::interactor::session_actor::actor::SessionContext;
use crate::interactor::session_actor::runtime::{LaunchTarget, PendingSpawn};
use crate::pane_token::PaneToken;
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};

/// What the launch task must do next, decided by the actor when it records the
/// pending spawn.
///
/// A two-state answer rather than a bare `bool` because the two states are
/// instructions to the caller, not a property of the session: the task reads
/// this to decide whether a tmux pane may be created at all, and a `false`
/// there would have to be re-explained at every call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::interactor) enum LaunchApproval {
    /// The pending spawn is recorded; create the pane.
    Proceed,
    /// There is no launch left to complete — the acceptance was already rolled
    /// back or the session was closed — so the pane must NOT be created: it
    /// would be an orphan process no session could ever bind or kill.
    Abandon,
}

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Turn this session's [`LaunchingSpawn`] into a [`PendingSpawn`], posted by
    /// the launch task ([`spawn_launch_preparation`]) once the preparation is
    /// done and *before* it creates the tmux pane.
    ///
    /// # Why this is its own round-trip
    ///
    /// The record the launch's first `SessionStart`/`UserPromptSubmit` binds is
    /// the [`PendingSpawn`], and those hooks arrive on this same mailbox. So the
    /// pending spawn must be recorded strictly before the pane exists —
    /// otherwise a fast agent (or a test double, which is instant) can submit
    /// its launch prompt while the record is still missing: the hook finds
    /// nothing pending, is dismissed as external input, and the spawn that is
    /// recorded a moment later has no hook left to bind it. The session then
    /// never activates.
    ///
    /// Doing it here — the task awaits this reply before calling
    /// `create_session` — makes that ordering structural rather than a race the
    /// launch usually wins: the pane cannot exist until this message has been
    /// applied, and every hook it triggers queues behind it.
    ///
    /// The bind watchdog's clock therefore starts here too, a beat before the
    /// pane comes up, rather than at acceptance: a long `git fetch` cannot eat
    /// the deadline the first hook has to arrive within.
    ///
    /// [`LaunchingSpawn`]: crate::interactor::session_actor::runtime::LaunchingSpawn
    /// [`spawn_launch_preparation`]: super::launch_prep::spawn_launch_preparation
    pub(in crate::interactor) fn record_launched_pane(
        &mut self,
        token: &PaneToken,
    ) -> LaunchApproval {
        let Some(launching) = self.state.take_launching_for_token(token) else {
            tracing::warn!(
                token = %token.as_str(),
                session_id = %self.id,
                "a prepared launch has no matching launching entry; abandoning it \
                 rather than creating a pane nothing can bind"
            );
            return LaunchApproval::Abandon;
        };
        // Only a pane launch posts this checkpoint (an adapter launch checks in
        // on `AdapterLaunchPrepared` instead), so a mismatch is a routing bug
        // rather than a race: put the entry back untouched and abandon, so no
        // pane is created and `LaunchFinished` still finds the entry to settle.
        if !matches!(launching.target, LaunchTarget::Pane(_)) {
            tracing::error!(
                token = %token.as_str(),
                session_id = %self.id,
                "an adapter-backed launch reported a pane checkpoint; abandoning it"
            );
            self.state.start_launching(launching);
            return LaunchApproval::Abandon;
        }
        let LaunchTarget::Pane(pane) = launching.target else {
            unreachable!("the guard above rejected every non-pane target")
        };
        tracing::info!(
            token = %token.as_str(),
            session_id = %self.id,
            workdir = %launching.workdir,
            prepared_in_ms = launching.accepted_at.elapsed().as_millis(),
            "launch preparation finished; recording the pending spawn before the pane"
        );
        self.state.push_pending(PendingSpawn {
            token: launching.token,
            pane: pane.pane,
            // Stamp just before the pane is created, not at acceptance: the
            // preparation that just finished may have taken far longer than the
            // bind deadline allows for, and the first hook can only fire from
            // here on.
            created_at: Instant::now(),
        });
        LaunchApproval::Proceed
    }
}
