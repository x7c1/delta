use delta_model::SessionId;

use crate::error::Result;
use crate::ports::{SessionStore, TmuxDriver, Transcript, Workspace};
use crate::Interactor;

impl<T, X, S, W> Interactor<T, X, S, W>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
{
    /// Mark a resumed session ready and dispatch its held first prompt.
    ///
    /// The release trigger for the resume readiness gate (see
    /// [`Self::open_session`]). On `SessionStart(source=resume)` the cold pane is
    /// now able to accept input, so this:
    ///
    /// 1. removes the session from the resuming map (so later sends dispatch
    ///    immediately, no gate), and
    /// 2. if a first prompt was held, types it into the resumed pane via the
    ///    normal [`TmuxDriver::send_line`] path — the same path every other send
    ///    takes, so the resume's first keystroke is no longer lost to a
    ///    still-cold TUI.
    ///
    /// The `pending_send` row for that prompt was already written (with its
    /// thread/branch/quote semantics) when the send was first enqueued; only the
    /// physical keystroke was held, so this dispatch completes a normal send.
    ///
    /// A no-op when the session is not resuming: the readiness hook for a session
    /// that already became ready, was never resumed under Delta, or carries no
    /// held prompt. This makes `SessionStart(source=resume)` idempotent and safe
    /// for a plain (no-immediate-send) resume.
    pub(in crate::interactor) async fn release_resumed_first_prompt(
        &self,
        session_id: &SessionId,
    ) -> Result<()> {
        let resuming = self.open_sessions.lock().await.mark_resume_ready(session_id);
        let Some(resuming) = resuming else {
            tracing::debug!(
                session_id = %session_id,
                "SessionStart(resume): session not resuming (already ready or not Delta-resumed); \
                 no held prompt to release"
            );
            return Ok(());
        };

        let Some(text) = resuming.held_prompt else {
            tracing::info!(
                session_id = %session_id,
                "SessionStart(resume): resume is ready with no held first prompt; nothing to \
                 dispatch"
            );
            return Ok(());
        };

        tracing::info!(
            session_id = %session_id,
            pane = %resuming.pane,
            "SessionStart(resume): resume ready, dispatching the held first prompt"
        );
        // The turn flag was set when the send was enqueued (its keystroke was
        // held, not its bookkeeping), so a dispatch failure here must clear it
        // and cancel the now-undeliverable row, mirroring the other dispatch
        // sites, so a failed release cannot wedge the FIFO.
        if let Err(err) = self.tmux.send_line(&resuming.pane, &text).await {
            tracing::warn!(
                session_id = %session_id,
                error = %err,
                "failed to dispatch the held resume first prompt; cancelling its pending send"
            );
            if let Some(head) = self.store.head_pending_send(session_id).await? {
                let _ = self.store.cancel_send(head.id).await;
            }
            let _ = self.store.set_turn_active(session_id, false).await;
            return Err(err);
        }
        Ok(())
    }
}
