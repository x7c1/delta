use delta_model::SessionId;

use crate::error::Result;
use crate::ports::{SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace};
use crate::turn::{TurnInput, TurnState};
use crate::Interactor;

impl<T, X, S, W> Interactor<T, X, S, W>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
{
    /// Dispatch the oldest `queued` send for a session, if the session's turn
    /// is idle, one is recorded, and the session has a live pane.
    ///
    /// A queued send is one that was composed while a turn was in flight and
    /// held back rather than dispatched mid-turn (which would make Claude Code
    /// queue it, losing the `UserPromptSubmit` hook that injects its locator
    /// quote). Once the turn returns to `Idle` this promotes the send to
    /// `dispatched` and types its keystrokes, so it submits as an ordinary
    /// prompt: the hook fires and the quote is injected normally, and the turn
    /// machine moves to `AwaitingEcho` so a following send defers behind it.
    ///
    /// **Single-outstanding rule**: this is the only place a queued send is
    /// promoted, and it only acts when the turn state is [`TurnState::Idle`] —
    /// so at most one `dispatched` send exists per session at any time, one
    /// per turn. The next queued send dispatches when this one's turn ends and
    /// the state returns to `Idle` (via the `Stop`/interrupt triggers calling
    /// back in here).
    ///
    /// A no-op (returning `None`) when the turn is not idle, there is no
    /// queued send, or the session has no live pane (closed) — in which case
    /// the send stays `queued` and is dispatched the next time the session is
    /// open and idle (see `enqueue_into_open`'s idle-flush). Promotes before
    /// dispatch so the outstanding row is in place when the hook fires; on a
    /// dispatch failure the `DispatchFailed` turn input cancels the row so a
    /// failed send cannot wedge the queue.
    ///
    /// Returns the [`SessionEvent::SendDispatched`] to broadcast when a send
    /// was promoted, so the browser sees the queued→dispatched transition
    /// immediately.
    pub(in crate::interactor) async fn dispatch_queued_send(
        &self,
        session_id: &SessionId,
    ) -> Result<Option<SessionEvent>> {
        if self.turn_state_for(session_id).await != TurnState::Idle {
            return Ok(None);
        }
        let Some(send) = self.store.next_queued_send(session_id).await? else {
            return Ok(None);
        };
        let Some(pane) = self.pane_for_session(session_id).await else {
            return Ok(None);
        };

        self.store.promote_queued_send(send.id).await?;
        self.apply_turn_input(session_id, TurnInput::Dispatch { send_id: send.id })
            .await?;
        if let Err(err) = self.tmux.send_line(&pane, &send.text).await {
            // The DispatchFailed transition cancels the orphaned row, so the
            // failed send cannot wedge the queue.
            self.apply_turn_input(session_id, TurnInput::DispatchFailed)
                .await?;
            return Err(err);
        }
        Ok(Some(SessionEvent::SendDispatched {
            session_id: session_id.clone(),
            send_id: send.id,
        }))
    }
}
