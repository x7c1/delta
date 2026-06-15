//! Feeding signals into the per-session turn state machine.
//!
//! The transitions themselves live in the `turn` module (one exhaustive
//! table); this file is the single place that *executes* a transition's side
//! effects — the orphaned-send disposition and the anomaly logging — so every
//! call site feeds the machine the same way. It runs inside the session's
//! actor, where the turn state is plain owned data: the mailbox already
//! serialized every input that can move it.

use crate::error::Result;
use crate::interactor::session_actor::actor::SessionContext;
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};
use crate::turn::{OrphanedSend, TurnInput, TurnState};

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Apply one input to the session's turn state machine, executing the
    /// transition's orphan disposition and logging anomalies. Returns the next
    /// state.
    pub(in crate::interactor) async fn apply_turn_input(
        &mut self,
        input: TurnInput,
    ) -> Result<TurnState> {
        let id = self.id;
        // Capture the source state before the table mutates it so the log line
        // records the full from -> to edge, not just the destination.
        let from = self.state.turn();
        let result = self.state.apply_turn(input);
        if result.anomalous {
            tracing::warn!(
                session_id = %id,
                from = ?from,
                trigger = ?input,
                to = ?result.next,
                orphaned = ?result.orphaned,
                "anomalous turn transition: this input should be impossible in the \
                 previous state; converging on the safest outcome"
            );
        } else {
            tracing::debug!(
                session_id = %id,
                from = ?from,
                trigger = ?input,
                to = ?result.next,
                "turn transition"
            );
        }

        match result.orphaned {
            None => {}
            Some(OrphanedSend::Requeue(send_id)) => {
                tracing::warn!(
                    session_id = %id,
                    send_id,
                    "outstanding send never echoed; returning it to `queued` so it \
                     re-dispatches when the session is next idle"
                );
                self.store.requeue_send(send_id).await?;
            }
            Some(OrphanedSend::Cancel(send_id)) => {
                tracing::warn!(
                    session_id = %id,
                    send_id,
                    "outstanding send can no longer be delivered; cancelling it"
                );
                self.store.cancel_send(send_id).await?;
            }
            Some(OrphanedSend::CancelIfUnmatched(send_id)) => {
                // The send normally matched its transcript line during the
                // turn (leaving `dispatched`); this defensive sweep only fires
                // when that line never appeared, so a stale `dispatched` row
                // cannot shadow the next dispatch's correlation.
                if let Some(head) = self.store.head_dispatched_send(self.id).await? {
                    if head.id == send_id {
                        tracing::warn!(
                            session_id = %self.id,
                            send_id,
                            "turn ended but its send never matched a transcript line; \
                             cancelling the stale dispatched row"
                        );
                        self.store.cancel_send(send_id).await?;
                    }
                }
            }
        }
        Ok(result.next)
    }
}
