use std::time::Instant;

use crate::error::Result;
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
    /// Dispatch this session's held first prompt if its resume is ready *and*
    /// has settled, on the background tick.
    ///
    /// This is the second stage of the resume readiness gate (see
    /// [`Self::open_session`]). It exists because `SessionStart(source=resume)`
    /// blocks `claude` until the hook handler returns: typing the held keystroke
    /// from inside that handler lands it while `claude` is still inside the hook
    /// and not accepting input, so it is silently lost (no `UserPromptSubmit`, the
    /// prompt never submits, and the resume is later reaped as a spurious
    /// `SpawnFailed`). So the readiness hook only *marks* the resume ready
    /// (`mark_resume_ready_at`, returning immediately to unblock `claude`), and
    /// the actual keystroke is dispatched here — on a periodic tick that runs
    /// outside any hook handler, after the hook has returned and `claude` is
    /// input-ready.
    ///
    /// When the resume's `now - ready_at` has reached
    /// [`RESUME_DISPATCH_SETTLE`], this takes it off the runtime state and, if a
    /// first prompt was held, types it into the resumed pane via the normal
    /// [`TmuxDriver::send_line`] path — the same path every other send takes. The
    /// `send` row for that first prompt was already written (with its
    /// thread/branch/quote semantics) when the send was first enqueued; only the
    /// physical keystroke was held, so this completes a normal send.
    ///
    /// On a `send_line` failure it mirrors the other dispatch sites so a failed
    /// dispatch cannot wedge the queue: the `DispatchFailed` turn input cancels
    /// the now-undeliverable outstanding send and returns the turn to idle. The
    /// failure is logged rather than propagated, so one pane's send failure
    /// cannot strand the other sessions' ticks.
    ///
    /// `now` is injected (rather than read here) so the dispatch is deterministic
    /// under test, mirroring the watchdog reap: the server loop passes
    /// `Instant::now()`, while tests advance a controlled instant.
    ///
    /// [`RESUME_DISPATCH_SETTLE`]: crate::interactor::session_actor::runtime::RESUME_DISPATCH_SETTLE
    pub(in crate::interactor) async fn dispatch_ready_resume(
        &mut self,
        now: Instant,
    ) -> Result<()> {
        let Some(resuming) = self.state.take_ready_for_dispatch(now) else {
            return Ok(());
        };

        let Some(text) = resuming.held_prompt else {
            tracing::info!(
                session_id = %self.id,
                "resume settled with no held first prompt; nothing to dispatch"
            );
            return Ok(());
        };

        tracing::info!(
            session_id = %self.id,
            pane = %resuming.pane,
            "resume settled after SessionStart(resume); dispatching the held first prompt"
        );
        // The turn machine moved to `AwaitingEcho` when the send was
        // enqueued (its keystroke was held, not its bookkeeping), so a
        // dispatch failure here must feed `DispatchFailed` — which cancels
        // the now-undeliverable row and returns the turn to idle,
        // mirroring the other dispatch sites, so a failed dispatch cannot
        // wedge the queue.
        if let Err(err) = self.tmux.send_line(&resuming.pane, &text).await {
            tracing::warn!(
                session_id = %self.id,
                error = %err,
                "failed to dispatch the held resume first prompt; cancelling its open send"
            );
            let _ = self
                .apply_turn_input(crate::turn::TurnInput::DispatchFailed)
                .await;
        }
        Ok(())
    }
}
