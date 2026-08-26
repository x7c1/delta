//! Releasing a *held* send back into the normal queued flow.
//!
//! Two paths leave a row `queued` + `held_at`, and this release is the way
//! out of both:
//!
//! - the boot-time reconcile (see
//!   [`SessionStore::restore_all_dispatched`](crate::ports::SessionStore::restore_all_dispatched)),
//!   which recovers every `dispatched` row a dead server process left behind;
//! - the echo-deadline park (see
//!   [`SessionStore::hold_send_for_release`](crate::ports::SessionStore::hold_send_for_release)),
//!   for a send whose keystrokes vanished without a trace twice running.
//!
//! Such a row is deliberately **never dispatched automatically**: a restored
//! message may be days old and the conversation has moved on, so silently
//! re-submitting it on the next reopen — possibly after a newer message the
//! user just sent — was rejected on review; and a parked one is worth re-typing
//! only once the user has dealt with whatever swallowed it. Instead the UI
//! shows the held row with two explicit actions: **Send** (this release) and
//! **Cancel** (the ordinary queued cancel, which already covers held rows
//! because their status is still `queued`).
//!
//! Releasing is ensure-open plus a guarded marker clear. The session is
//! first ensured open ([`ensure_open`](super::enqueue)) — resumed via
//! `claude --resume <id>` when it is known but closed, exactly as an
//! ordinary enqueue would — because a restored row typically belongs to a
//! session the restart left closed, and with no live pane nothing else
//! would ever dispatch (or even reopen) it. Then the marker clears
//! ([`SessionStore::release_held_send`](crate::ports::SessionStore::release_held_send)):
//! only a still-`queued`, still-held row transitions, so a release racing
//! a cancel (or a duplicate release) is a clean
//! [`Error::SendNotReleasable`] conflict rather than a clobber. On success
//! the row is an ordinary `queued` send again, and the tail call into
//! [`dispatch_queued_send`](super::enqueue) types it immediately when the
//! session was already open and idle — the same path every queued send
//! takes. When the release itself resumed the session the row instead waits
//! out the resume-readiness window and is flushed by the resume-settle tick;
//! when a turn is mid-flight it waits for the turn-end trigger.
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
    /// Release a held send of this session into the normal queued flow,
    /// ensuring the session is open first (resumed when it is known but
    /// closed) so the released row cannot strand paneless.
    ///
    /// Returns [`Error::SendNotReleasable`] when no row transitioned: the
    /// send is unknown, was never held, is already released, or has been
    /// cancelled since. The browser drops its Send control and reconciles
    /// from the next refetch on this error. An ensure-open failure (e.g.
    /// [`Error::ResumeUnavailable`] for a gone transcript, or a spawn error)
    /// surfaces *before* the marker is touched, so the row stays held and
    /// the release can be retried.
    ///
    /// After a successful release the session's queued dispatch runs exactly
    /// as it does after a cancel clears a head: if the session is open and
    /// idle the oldest unheld `queued` send (FIFO — which may be an even
    /// older row released or composed earlier) is promoted and typed, and the
    /// resulting [`SessionEvent::SendDispatched`] is returned for the caller
    /// to broadcast. Otherwise the released row simply waits as a normal
    /// queued send for the next dispatch trigger — for a session this
    /// release just resumed, that is the resume-settle flush.
    pub(in crate::interactor) async fn release_send(
        &mut self,
        send_id: i64,
    ) -> Result<Option<SessionEvent>> {
        // Ensure the session is open before touching the row — mirroring
        // `enqueue_to_thread`, which ensures the target open before its store
        // mutation. Restored rows exist precisely because the server
        // restarted, so the common case here is a closed session: without
        // this resume the tail dispatch below would no-op (no live pane) and
        // no later trigger would ever fire — the released row would strand
        // as an ordinary `queued` send of a session nothing reopens. When
        // the session is already open this is a plain pane lookup.
        self.ensure_open().await?;
        if !self.store.release_held_send(send_id).await? {
            return Err(Error::SendNotReleasable(send_id));
        }
        // The released row is ordinary `queued` now; give it the same
        // immediate chance every other queued send gets. `dispatch_queued_send`
        // no-ops when the turn is busy or the resume window (including one
        // the ensure-open above just opened) has not settled — in those
        // cases the row waits for the next trigger (turn end, or the
        // resume-settle flush) instead.
        self.dispatch_queued_send().await
    }
}
