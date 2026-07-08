//! Releasing a *restored* send back into the normal queued flow.
//!
//! The boot-time reconcile (see
//! [`SessionStore::restore_all_dispatched`](crate::ports::SessionStore::restore_all_dispatched))
//! recovers every `dispatched` row a dead server process left behind as
//! `queued` + `restored_at`. Such a row is deliberately **never dispatched
//! automatically**: the message may be days old and the conversation has
//! moved on, so silently re-submitting it on the next reopen — possibly after
//! a newer message the user just sent — was rejected on review. Instead the
//! UI shows the restored row with two explicit actions: **Send** (this
//! release) and **Cancel** (the ordinary queued cancel, which already covers
//! restored rows because their status is still `queued`).
//!
//! Releasing is a guarded marker clear
//! ([`SessionStore::release_restored_send`](crate::ports::SessionStore::release_restored_send)):
//! only a still-`queued`, still-restored row transitions, so a release racing
//! a cancel (or a duplicate release) is a clean
//! [`Error::SendNotReleasable`] conflict rather than a clobber. On success
//! the row is an ordinary `queued` send again, and the tail call into
//! [`dispatch_queued_send`](super::enqueue) types it immediately when the
//! session is open and idle — the same path every queued send takes — or
//! leaves it waiting for the next dispatch trigger when the session is
//! closed, resuming, or mid-turn.
//!
//! Routed through the owning session's actor (resolved from the send id in
//! [`release_send`](crate::interactor::Interactor::release_send)) so the
//! release is ordered against that session's dispatch path, mirroring
//! [`cancel_send`](super::cancel_send).

use crate::error::{Error, Result};
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
    /// Release a restored send of this session into the normal queued flow.
    ///
    /// Returns [`Error::SendNotReleasable`] when no row transitioned: the
    /// send is unknown, was never restored, is already released, or has been
    /// cancelled since. The browser drops its Send control and reconciles
    /// from the next refetch on this error.
    ///
    /// After a successful release the session's queued dispatch runs exactly
    /// as it does after a cancel clears a head: if the session is open and
    /// idle the oldest unrestored `queued` send (FIFO — which may be an even
    /// older row released or composed earlier) is promoted and typed, and the
    /// resulting [`SessionEvent::SendDispatched`] is returned for the caller
    /// to broadcast. Otherwise the released row simply waits as a normal
    /// queued send for the next dispatch trigger.
    pub(in crate::interactor) async fn release_send(
        &mut self,
        send_id: i64,
    ) -> Result<Option<SessionEvent>> {
        if !self.store.release_restored_send(send_id).await? {
            return Err(Error::SendNotReleasable(send_id));
        }
        // The released row is ordinary `queued` now; give it the same
        // immediate chance every other queued send gets. `dispatch_queued_send`
        // no-ops when the turn is busy, the resume window is open, or the
        // session has no live pane — in those cases the row waits for the
        // next trigger instead.
        self.dispatch_queued_send().await
    }
}
