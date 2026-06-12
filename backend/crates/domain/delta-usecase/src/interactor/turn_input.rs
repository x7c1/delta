//! Feeding signals into the per-session turn state machine.
//!
//! The transitions themselves live in the `turn` module (one exhaustive
//! table); this file is the single place that *executes* a transition's side
//! effects — the orphaned-send disposition and the anomaly logging — so every
//! call site feeds the machine the same way.

use delta_model::SessionId;

use crate::error::Result;
use crate::ports::{SessionStore, TmuxDriver, Transcript, Workspace};
use crate::turn::{OrphanedSend, TurnInput, TurnState};
use crate::Interactor;

impl<T, X, S, W> Interactor<T, X, S, W>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
{
    /// The current turn state of a session.
    ///
    /// Public because the REST surface reports it (the sends envelope carries
    /// `turn`, so the browser can rebuild its in-progress indicator after a
    /// reconnect).
    pub async fn turn_state_for(&self, id: &SessionId) -> TurnState {
        self.turns.lock().await.state(id)
    }

    /// Apply one input to a session's turn state machine, executing the
    /// transition's orphan disposition and logging anomalies. Returns the next
    /// state.
    ///
    /// The registry lock is held only for the transition itself; the orphan's
    /// store write runs after it is released.
    pub(in crate::interactor) async fn apply_turn_input(
        &self,
        id: &SessionId,
        input: TurnInput,
    ) -> Result<TurnState> {
        let result = {
            let mut turns = self.turns.lock().await;
            turns.apply(id, input)
        };
        if result.anomalous {
            tracing::warn!(
                session_id = %id,
                ?input,
                next = ?result.next,
                orphaned = ?result.orphaned,
                "anomalous turn transition: this input should be impossible in the \
                 previous state; converging on the safest outcome"
            );
        } else {
            tracing::debug!(session_id = %id, ?input, next = ?result.next, "turn transition");
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
                if let Some(head) = self.store.head_dispatched_send(id).await? {
                    if head.id == send_id {
                        tracing::warn!(
                            session_id = %id,
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

    /// Drop a session's turn state without orphan handling, for paths that
    /// delete the session row outright (its sends are removed by cascade, so
    /// there is no row left to requeue or cancel).
    pub(in crate::interactor) async fn forget_turn(&self, id: &SessionId) {
        self.turns.lock().await.forget(id);
    }
}
