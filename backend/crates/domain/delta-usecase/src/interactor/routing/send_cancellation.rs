//! Send cancellation and release: both requests carry only a send id, so the
//! owning session is derived from the send row here before the work executes
//! on that session's actor, ordered against its dispatch path.

use crate::error::{Error, Result};
use crate::interactor::session_actor::input::SessionInput;
use crate::interactor::Interactor;
use crate::ports::{GitWorktree, SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace};

impl<T, X, S, W, G> Interactor<T, X, S, W, G>
where
    T: TmuxDriver + 'static,
    X: Transcript + 'static,
    S: SessionStore + 'static,
    W: Workspace + 'static,
    G: GitWorktree + 'static,
{
    /// Cancel a `queued` send before it is dispatched, or a `dispatched` send
    /// whose echo has not arrived (typically the user pressed `Escape` in the
    /// TUI to discard the composer buffer, leaving no signal Delta can
    /// observe — see the module doc on
    /// [`cancel_send`](crate::interactor::cancel_send)).
    ///
    /// The cancel request carries only the send id (in its URL), so the
    /// owning session is derived from the send row here — mirroring how
    /// [`enqueue_send`](Self::enqueue_send) derives the session from a thread
    /// — and the cancel then executes on that session's actor, ordered
    /// against its dispatch path.
    ///
    /// Cancelling a `dispatched` send the turn machine is awaiting injects a
    /// single `Escape` into the pane (the same gesture
    /// [`cancel_question`](Self::cancel_question) uses) and promotes any
    /// queued send behind the cancelled head through the existing idle-flush
    /// path. A `dispatched` row the turn machine holds no claim on is
    /// cancelled as a pure state transition — no keystrokes, no turn input
    /// (see the module doc on ownerless rows).
    ///
    /// Returns [`Error::SendNotCancellable`] (`409`) when the send no longer
    /// exists, is already terminal (matched a transcript line, or already
    /// cancelled), or is `dispatched` but its echo has already arrived — the
    /// turn carries it `InFlight`, owned by its transcript line, and the
    /// user reaches for the in-flight interrupt instead. The browser drops
    /// its cancel control and reconciles from the next refetch on this
    /// error.
    pub async fn cancel_send(&self, send_id: i64) -> Result<()> {
        let Some(send) = self.store.send(send_id).await? else {
            return Err(Error::SendNotCancellable(send_id));
        };
        let session_id = send.session_id;
        self.request(&session_id, move |reply| SessionInput::CancelSend {
            send_id,
            reply,
        })
        .await
    }

    /// Release a *held* send — one the boot-time reconcile recovered from a
    /// dead process's `dispatched` state, or one the echo deadline parked —
    /// back into the normal queued flow (see the module doc on
    /// [`release_send`](crate::interactor::release_send)).
    ///
    /// Like a cancel, the release request carries only the send id (in its
    /// URL), so the owning session is derived from the send row here and the
    /// release then executes on that session's actor, ordered against its
    /// dispatch path. The actor first ensures the session is open — resuming
    /// it via `claude --resume <id>` when it is closed, the normal state
    /// right after the restart that produced a boot-restored row — exactly as
    /// an enqueue would. When the session was already open and idle the
    /// released row dispatches immediately through the normal queued path;
    /// the returned [`SessionEvent`]s (a `SendDispatched`, when that
    /// happened) are broadcast by the transport so the browser sees the
    /// transition. When the release itself resumed the session the row waits
    /// out the resume-readiness window and is typed by the resume-settle
    /// flush ([`Self::dispatch_ready_resumes`]).
    ///
    /// Returns [`Error::SendNotReleasable`] (`409`) when the send is
    /// unknown, was never held, is already released, or has since been
    /// cancelled. The browser drops its Send control and reconciles from the
    /// next refetch on this error. An ensure-open failure — e.g.
    /// [`Error::ResumeUnavailable`] when the session's transcript is gone —
    /// surfaces as-is, before the hold marker is touched, so the release can
    /// be retried.
    pub async fn release_send(&self, send_id: i64) -> Result<Vec<SessionEvent>> {
        let Some(send) = self.store.send(send_id).await? else {
            return Err(Error::SendNotReleasable(send_id));
        };
        let session_id = send.session_id;
        let dispatched = self
            .request(&session_id, move |reply| SessionInput::ReleaseSend {
                send_id,
                reply,
            })
            .await?;
        Ok(dispatched.into_iter().collect())
    }
}
