use std::time::Instant;

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
    /// Dispatch the held first prompt of every resume that is ready *and* has
    /// settled, on the background tick.
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
    /// For each resume whose `now - ready_at` has reached
    /// [`RESUME_DISPATCH_SETTLE`], this drains it from the resuming map and, if a
    /// first prompt was held, types it into the resumed pane via the normal
    /// [`TmuxDriver::send_line`] path — the same path every other send takes. The
    /// `pending_send` row for that prompt was already written (with its
    /// thread/branch/quote semantics) when the send was first enqueued; only the
    /// physical keystroke was held, so this completes a normal send.
    ///
    /// On a `send_line` failure it mirrors the other dispatch sites so a failed
    /// dispatch cannot wedge the FIFO: it cancels the now-undeliverable head
    /// pending send and clears the turn-active flag. The failure is logged and the
    /// loop continues to the next ready resume rather than aborting the whole tick.
    ///
    /// `now` is injected (rather than read here) so the dispatch is deterministic
    /// under test, mirroring [`Self::reap_stale_spawns`]: the server loop passes
    /// `Instant::now()`, while tests advance a controlled instant.
    ///
    /// [`RESUME_DISPATCH_SETTLE`]: crate::open_sessions::RESUME_DISPATCH_SETTLE
    pub async fn dispatch_ready_resumes(&self, now: Instant) -> Result<()> {
        // Take the registry lock only long enough to drain the settled resumes;
        // the per-pane `send_line` below runs without the lock so it cannot
        // serialize the hooks or the PTY bridge against per-pane I/O.
        let ready = {
            let mut registry = self.open_sessions.lock().await;
            registry.drain_ready_for_dispatch(now)
        };

        for (session_id, resuming) in ready {
            let Some(text) = resuming.held_prompt else {
                tracing::info!(
                    session_id = %session_id,
                    "resume settled with no held first prompt; nothing to dispatch"
                );
                continue;
            };

            tracing::info!(
                session_id = %session_id,
                pane = %resuming.pane,
                "resume settled after SessionStart(resume); dispatching the held first prompt"
            );
            // The turn flag was set when the send was enqueued (its keystroke was
            // held, not its bookkeeping), so a dispatch failure here must clear it
            // and cancel the now-undeliverable row, mirroring the other dispatch
            // sites, so a failed dispatch cannot wedge the FIFO.
            if let Err(err) = self.tmux.send_line(&resuming.pane, &text).await {
                tracing::warn!(
                    session_id = %session_id,
                    error = %err,
                    "failed to dispatch the held resume first prompt; cancelling its pending send"
                );
                if let Some(head) = self.store.head_pending_send(&session_id).await? {
                    let _ = self.store.cancel_send(head.id).await;
                }
                let _ = self.store.set_turn_active(&session_id, false).await;
                // Keep dispatching the remaining ready resumes: one pane's send
                // failure must not strand the others queued on this tick.
                continue;
            }
        }
        Ok(())
    }
}
