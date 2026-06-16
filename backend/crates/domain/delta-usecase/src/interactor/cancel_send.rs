//! Cancelling a still-`queued` send from the browser before it is dispatched.
//!
//! A send composed while the assistant's turn is in flight is held back in the
//! `queued` state and only dispatched once the session goes idle (see
//! [`dispatch_queued_send`](super::enqueue)). Until that dispatch the send has
//! not touched the pane, so cancelling it is a pure state transition: flip the
//! row to `SendStatus::Cancelled` and the idle dispatch path
//! ([`next_queued_send`](crate::ports::SessionStore::next_queued_send), which
//! filters on `status = 'queued'`) will simply skip it, and it drops out of the
//! browser's open-send list.
//!
//! Scope is deliberately **`queued` only**. A `dispatched` send's keystrokes
//! are already in the pane's composer and entangled with the turn machine's
//! echo/match correlation, so cancelling one cleanly would mean injecting an
//! `Escape` and reconciling the FIFO head — out of scope for this MVP. The
//! guard lives in the store
//! ([`cancel_queued_send`](crate::ports::SessionStore::cancel_queued_send),
//! `WHERE status = 'queued'`), so even if the send is dispatched the instant
//! between the browser's click and this handler, the transition is a no-op and
//! the caller learns the send is no longer cancellable rather than clobbering an
//! in-flight send.
//!
//! Routed through the owning session's actor (resolved from the send id in
//! [`cancel_send`](crate::interactor::Interactor::cancel_send)) so the cancel is
//! ordered against that session's dispatch path, mirroring how every other
//! send-state transition runs inside the actor.

use crate::error::{Error, Result};
use crate::interactor::session_actor::actor::SessionContext;
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Cancel a still-`queued` send of this session.
    ///
    /// Returns [`Error::SendNotCancellable`] when no row transitioned: the send
    /// has already left `queued` (dispatched, matched, or cancelled) or never
    /// existed. The store's guarded transition is the single source of truth for
    /// "still cancellable", so this method does not re-check the status itself —
    /// it just maps a no-op transition onto the conflict error.
    pub(in crate::interactor) async fn cancel_send(&mut self, send_id: i64) -> Result<()> {
        if self.store.cancel_queued_send(send_id).await? {
            Ok(())
        } else {
            Err(Error::SendNotCancellable(send_id))
        }
    }
}
