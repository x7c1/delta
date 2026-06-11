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
    /// Dispatch the oldest `deferred` send for a session, if one is queued and
    /// the session has a live pane.
    ///
    /// A deferred send is one that was composed while a turn was in flight and
    /// held back rather than dispatched mid-turn (which would make Claude Code
    /// queue it, losing the `UserPromptSubmit` hook that injects its locator
    /// quote). Once the session goes idle this promotes the send to `pending`
    /// and types its keystrokes, so it submits as an ordinary prompt: the hook
    /// fires and the quote is injected normally, and the turn flag is set so a
    /// following branch/quoted send defers behind it.
    ///
    /// A no-op when there is no deferred send, or the session has no live pane
    /// (closed) — in which case the send stays `deferred` and is dispatched the
    /// next time the session is open and idle (see `enqueue_into_open`'s
    /// idle-flush). Promotes before dispatch so the correlation head is in place
    /// when the hook fires; on a dispatch failure it rolls the row back and
    /// clears the turn flag so a failed send cannot wedge the queue.
    pub(in crate::interactor) async fn dispatch_deferred_send(
        &self,
        session_id: &SessionId,
    ) -> Result<()> {
        let Some(send) = self.store.next_deferred_send(session_id).await? else {
            return Ok(());
        };
        let Some(pane) = self.pane_for_session(session_id).await else {
            return Ok(());
        };

        self.store.promote_deferred_send(send.id).await?;
        self.store.set_turn_active(session_id, true).await?;
        if let Err(err) = self.tmux.send_line(&pane, &send.text).await {
            let _ = self.store.cancel_send(send.id).await;
            let _ = self.store.set_turn_active(session_id, false).await;
            return Err(err);
        }
        Ok(())
    }
}
