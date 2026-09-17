//! The reap tick: the two liveness checks the background sweep runs against one
//! session, on one cadence.
//!
//! The first is the launch watchdog ([`reap_stale_launch`]) — a launch that
//! never became ready before its deadline. The second is the pane probe
//! ([`close_if_pane_vanished`]) — an *already open* session whose tmux pane is
//! no longer there. Each check lives in its own module; this one holds only
//! their composition and the reason it is a composition.
//!
//! They share a tick rather than each owning a timer because they ask the same
//! question of the same port (`has_session`) at the same rate, and a second
//! timer in the server would only add a second thing to reason about when
//! sweeps overlap. The cost is one `tmux has-session` per *open* session per
//! tick — a short-lived subprocess, so dearer than the transcript tail's file
//! read on that same tick, but cheap enough that trading it for a slower probe
//! (and a session that reads as open for seconds after its agent died) is not
//! worth the extra state a separate cadence would need.
//!
//! [`reap_stale_launch`]: SessionContext::reap_stale_launch
//! [`close_if_pane_vanished`]: SessionContext::close_if_pane_vanished

use std::time::Instant;

use crate::error::Result;
use crate::interactor::session_actor::actor::SessionContext;
use crate::ports::{GitWorktree, SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace};

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Run both of the tick's checks against this session, returning every
    /// event they produced for the caller to broadcast.
    ///
    /// The launch watchdog goes first, and the two can never act on the same
    /// session in one tick: a launch that never bound holds no pane handle for
    /// the probe to find, and a resume that bound but is not ready yet is one
    /// the probe skips — if the watchdog reaps that resume here it takes the
    /// binding with it (`SessionRuntime::take_stale_resuming`), so the probe
    /// finds nothing bound either way.
    pub(in crate::interactor) async fn reap_tick(
        &mut self,
        now: Instant,
    ) -> Result<Vec<SessionEvent>> {
        let mut events = self.reap_stale_launch(now).await?;
        events.extend(self.close_if_pane_vanished().await?);
        Ok(events)
    }
}
