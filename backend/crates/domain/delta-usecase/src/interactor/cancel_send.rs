//! Cancelling a still-`queued` send held back from dispatch, or a `dispatched`
//! send whose echo never arrived (e.g. because the user pressed `Escape` in the
//! TUI to discard the composer buffer).
//!
//! ## Queued
//!
//! A send composed while the assistant's turn is in flight is held back in the
//! `queued` state and only dispatched once the session goes idle (see
//! [`dispatch_queued_send`](super::enqueue)). Until that dispatch the send has
//! not touched the pane, so cancelling it is a pure state transition: flip the
//! row to `SendStatus::Cancelled` and the idle dispatch path
//! ([`next_queued_send`](crate::ports::SessionStore::next_queued_send), which
//! filters on `status = 'queued'`) will simply skip it, and it drops out of the
//! browser's open-send list. The store guards the transition with
//! `WHERE status = 'queued'` ([`cancel_queued_send`](crate::ports::SessionStore::cancel_queued_send)),
//! so a send that left `queued` the instant between the browser's click and
//! this handler is a clean conflict rather than a clobber.
//!
//! ## Dispatched
//!
//! A `dispatched` send's keystrokes are already in the pane's composer and the
//! turn machine is in [`TurnState::AwaitingEcho`] expecting the
//! `UserPromptSubmit` echo. The classic stuck-forever case (the bug this path
//! fixes): the user pressed `Escape` in the TUI before the prompt submitted,
//! discarding the composer buffer — no echo will ever arrive, so the row
//! stays `dispatched` indefinitely and the composer is locked. This handler is
//! the escape hatch: it injects a single `Escape` keystroke into the pane
//! (mirroring how [`cancel_question`](super::cancel_question) cancels an
//! `AskUserQuestion`), drops the row to `cancelled`, and feeds the turn
//! machine a [`TurnInput::Cancel`] that exits `AwaitingEcho` back to
//! [`TurnState::Idle`] — at which point any queued sends behind the cancelled
//! head dispatch naturally on the next idle-flush (the existing
//! [`dispatch_queued_send`](super::enqueue) path).
//!
//! Cancel of a `dispatched` send is only honoured while the turn is in
//! `AwaitingEcho{send_id}` matching this send: once the echo arrives the turn
//! moves to [`TurnState::InFlight`] and is owned by its transcript line, so a
//! cancel there is rejected as [`Error::SendNotCancellable`] (the browser
//! reconciles from the refetch, and the user reaches for the existing
//! in-flight interrupt). The actor's mailbox serializes these checks against
//! the `UserPromptSubmit` hook that would move the state, so the test is
//! race-free.
//!
//! ## Late echo race
//!
//! The user might press Enter in the TUI a moment before the browser's cancel
//! lands. In that case the prompt has already submitted, but the
//! `UserPromptSubmit` hook fires *after* this handler has cancelled the row
//! and dropped state back to Idle. The hook's
//! [`head_dispatched_send`](crate::ports::SessionStore::head_dispatched_send)
//! query returns `None` (the cancelled row is filtered), so the prompt
//! classifies as [`TurnInput::ExternalPrompt`] — exactly the existing
//! treatment for an untracked external prompt. No stuck state, no panic.
//!
//! Routed through the owning session's actor (resolved from the send id in
//! [`cancel_send`](crate::interactor::Interactor::cancel_send)) so the cancel
//! is ordered against that session's dispatch path, mirroring how every other
//! send-state transition runs inside the actor.
//!
//! [`TurnInput::Cancel`]: crate::turn::TurnInput::Cancel
//! [`TurnInput::ExternalPrompt`]: crate::turn::TurnInput::ExternalPrompt
//! [`TurnState::AwaitingEcho`]: crate::turn::TurnState::AwaitingEcho
//! [`TurnState::Idle`]: crate::turn::TurnState::Idle
//! [`TurnState::InFlight`]: crate::turn::TurnState::InFlight

use delta_model::SendStatus;

use crate::error::{Error, Result};
use crate::interactor::question_keys::cancel_keys;
use crate::interactor::session_actor::actor::SessionContext;
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};
use crate::turn::{TurnInput, TurnState};

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Cancel a `queued` or `dispatched` send of this session.
    ///
    /// Returns [`Error::SendNotCancellable`] when no row transitioned: the
    /// send is unknown, has already left `queued`/`dispatched`, or is
    /// `dispatched` but the turn is no longer in `AwaitingEcho{send_id}`
    /// matching it (the echo already arrived, so the turn is owned by the
    /// in-flight line). The browser drops its cancel control and reconciles
    /// from the next refetch on this error.
    ///
    /// Cancelling a `dispatched` send injects a single `Escape` keystroke
    /// into the pane (discarding the TUI composer's typed-but-not-yet-
    /// submitted buffer) and then promotes any queued send behind the
    /// cancelled one through the existing idle-flush path — so a queue
    /// stacked behind the cancelled head proceeds naturally without any
    /// separate broadcast.
    pub(in crate::interactor) async fn cancel_send(&mut self, send_id: i64) -> Result<()> {
        let Some(send) = self.store.send(send_id).await? else {
            return Err(Error::SendNotCancellable(send_id));
        };
        match send.status {
            SendStatus::Queued => {
                if self.store.cancel_queued_send(send_id).await? {
                    Ok(())
                } else {
                    // Lost the race with idle dispatch: the row left `queued`
                    // between the browser's click and this guarded UPDATE.
                    // Map to the same conflict the queued-only path returned,
                    // so the browser refetches and reconciles.
                    Err(Error::SendNotCancellable(send_id))
                }
            }
            SendStatus::Dispatched => self.cancel_dispatched_send(send_id).await,
            SendStatus::Matched | SendStatus::Cancelled => {
                Err(Error::SendNotCancellable(send_id))
            }
        }
    }

    /// Cancel a `dispatched` send whose echo has not arrived.
    ///
    /// The turn must be in [`TurnState::AwaitingEcho`] with `send_id`
    /// matching: any other state means the echo already landed (the turn is
    /// in flight) or the send was already orphaned by another transition, in
    /// which case the cancel is rejected and the browser reconciles.
    async fn cancel_dispatched_send(&mut self, send_id: i64) -> Result<()> {
        match self.state.turn() {
            TurnState::AwaitingEcho { send_id: outstanding } if outstanding == send_id => {}
            _ => return Err(Error::SendNotCancellable(send_id)),
        }
        // The pane must be live to receive the Escape. A `dispatched` send
        // without a live pane would be a corrupted invariant (dispatch typed
        // into SOME pane), but be defensive: treat it as a conflict and let
        // the browser reconcile rather than panicking.
        let Some(pane) = self.state.handle().map(|handle| handle.pane.clone()) else {
            return Err(Error::SendNotCancellable(send_id));
        };

        // Mirror cancel_question: inject the same single Escape keystroke.
        // The TUI treats it as "discard the composer buffer", which is
        // exactly the user gesture we are reproducing on their behalf.
        let keys = cancel_keys();
        let names: Vec<&str> = keys.iter().map(|key| key.tmux_name()).collect();
        self.tmux.send_keys(&pane, &names).await?;

        // Drive the turn machine: AwaitingEcho{send_id} → Idle with
        // OrphanedSend::Cancel(send_id). The apply layer marks the row
        // `cancelled` through `store.cancel_send` as part of the orphan
        // dispatch — one path for "send is now terminally cancelled".
        self.apply_turn_input(TurnInput::Cancel { send_id }).await?;

        // FIFO head reconciliation: with the cancelled head out of the way
        // and the turn back to Idle, promote any send queued behind it. This
        // is the same path the Stop/interrupt handlers use; the broadcast
        // event is discarded because the browser refetches the open-send
        // list on the mutation, and the SendDispatched bookkeeping is
        // recovered from that refetch.
        let _ = self.dispatch_queued_send().await?;
        Ok(())
    }
}
