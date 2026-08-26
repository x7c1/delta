use crate::error::Result;
use crate::interactor::session_actor::actor::SessionContext;
use crate::ports::{GitWorktree, SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace};
use crate::turn::{TurnInput, TurnState};

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Dispatch the session's oldest `queued` send, if its turn is idle, one
    /// is recorded, and the session has a live pane.
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
    /// A no-op (returning `None`) when the turn is not idle, the session is
    /// still inside its resume-readiness window, there is no queued send, or
    /// the session has no live pane (closed) — in which case the send stays
    /// `queued` and is dispatched by the next trigger that reaches this
    /// method: a turn end (`Stop`), an interrupt ingest, a resume settle
    /// (`dispatch_ready_resume`), a dispatched-send cancellation, a
    /// held-send release (`release_send`), or `enqueue_into_open`'s
    /// idle-flush. *Held* rows — those carrying `held_at`, whether the
    /// boot restore recovered them from a dead process's `dispatched` state or
    /// the echo deadline parked them — are invisible to this method entirely:
    /// [`SessionStore::next_queued_send`] filters them out until the user
    /// explicitly releases them, so no trigger here can auto-resend a
    /// possibly-stale message or re-type one the pane keeps swallowing.
    /// Promotes before dispatch so the outstanding
    /// row is in place when the hook fires; on a dispatch failure the
    /// `DispatchFailed` turn input cancels the row so a failed send cannot
    /// wedge the queue.
    ///
    /// [`SessionStore::next_queued_send`]: crate::ports::SessionStore::next_queued_send
    ///
    /// Returns the [`SessionEvent::SendDispatched`] to broadcast when a send
    /// was promoted, so the browser sees the queued→dispatched transition
    /// immediately.
    pub(in crate::interactor) async fn dispatch_queued_send(
        &mut self,
    ) -> Result<Option<SessionEvent>> {
        if self.state.turn() != TurnState::Idle {
            return Ok(None);
        }
        // Resume-readiness guard: while the session is inside its resume
        // window the pane is bound but `claude` is not yet accepting input, so
        // a keystroke typed now would be silently lost (no `UserPromptSubmit`
        // fires and the promoted row would be stuck awaiting an echo that
        // never comes). Defer instead — a row deferred here is picked up at
        // resume settle, when `dispatch_ready_resume` calls back in.
        if self.state.is_resuming() {
            return Ok(None);
        }
        let Some(send) = self.store.next_queued_send(self.id).await? else {
            return Ok(None);
        };
        let Some(pane) = self.state.handle().map(|h| h.pane.clone()) else {
            return Ok(None);
        };

        self.store.promote_queued_send(send.id).await?;
        self.apply_turn_input(TurnInput::Dispatch { send_id: send.id })
            .await?;
        if let Err(err) = self.tmux.send_line(&pane, &send.text).await {
            // The DispatchFailed transition cancels the orphaned row, so the
            // failed send cannot wedge the queue.
            self.apply_turn_input(TurnInput::DispatchFailed).await?;
            return Err(err);
        }
        Ok(Some(SessionEvent::SendDispatched {
            session_id: self.id.clone(),
            send_id: send.id,
        }))
    }
}
