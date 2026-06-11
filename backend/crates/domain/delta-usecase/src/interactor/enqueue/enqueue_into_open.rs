use delta_model::{MessageUuid, PendingSend, SessionId, ThreadId};

use crate::error::Result;
use crate::ports::{SessionStore, TmuxDriver, Transcript, Workspace};
use crate::Interactor;

use super::provisional_branch_title;

impl<T, X, S, W> Interactor<T, X, S, W>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
{
    /// Write the `pending_send` row and dispatch the keystrokes for an open
    /// session, with the cancel-on-dispatch-failure rollback.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::interactor::enqueue) async fn enqueue_into_open(
        &self,
        session_id: &SessionId,
        pane: &str,
        thread_id: ThreadId,
        text: &str,
        locator_quote: Option<&str>,
        branch_from: Option<&MessageUuid>,
    ) -> Result<PendingSend> {
        // Idle-flush safety net: if the session is idle but a send is still
        // `deferred` (a dispatch trigger was missed, e.g. an interrupt the tail
        // had not tailed yet), release it now so it keeps its place ahead of
        // this new send in FIFO order. Dispatching it sets the turn flag, which
        // the defer check below then observes.
        if !self.store.is_turn_active(session_id).await? {
            self.dispatch_deferred_send(session_id).await?;
        }

        // The target thread was already loaded by the caller to derive the
        // session, so its existence is established here (a stale/wrong id surfaced
        // as `ThreadNotFound` before reaching this point).
        let (target_thread, semantic_parent) = match branch_from {
            Some(parent) => {
                // Give the new branch child a provisional title derived from the
                // locator quote so the navigator shows something meaningful
                // until it is renamed. Fall back to "untitled" when there is no
                // quote.
                let title = provisional_branch_title(locator_quote);
                let thread = self
                    .store
                    .create_thread(session_id, &title, Some(thread_id), Some(parent))
                    .await?;
                (thread.id, Some(parent.clone()))
            }
            None => (thread_id, None),
        };

        // Defer this send when a turn is in flight AND it carries thread context
        // (a branch entry or a locator quote). Dispatching mid-turn would make
        // Claude Code queue it, and a queued prompt fires no `UserPromptSubmit`
        // hook — so its locator quote would never be injected. Instead record it
        // as `deferred` (the branch child thread and the queued text persist) and
        // let the turn-end triggers dispatch it as an ordinary prompt once idle.
        // Plain main-line sends are not deferred: they need no quote, so Claude's
        // own mid-turn queueing is harmless for them.
        let carries_context = branch_from.is_some() || locator_quote.is_some();
        if carries_context && self.store.is_turn_active(session_id).await? {
            return self
                .store
                .enqueue_deferred_send(
                    session_id,
                    target_thread,
                    semantic_parent.as_ref(),
                    text,
                    locator_quote,
                )
                .await;
        }

        let pending = self
            .store
            .enqueue_send(
                session_id,
                target_thread,
                semantic_parent.as_ref(),
                text,
                locator_quote,
            )
            .await?;

        // If the keystrokes never reach the pane, the row we just wrote would
        // sit at the head of the FIFO forever and block all future
        // `UserPromptSubmit` correlation. Roll it back to `cancelled` so the
        // head clears, then surface the original dispatch error.
        //
        // Best-effort: if the rollback itself fails we keep the dispatch error
        // (the caller's actionable failure) rather than masking it with a store
        // error. We do *not* roll back the just-created branch child thread: an
        // empty, unnamed thread is harmless overlay data and may legitimately be
        // reused by a retry, whereas the FIFO-blocking pending row is the actual
        // hazard this guard exists to clear.
        if let Err(dispatch_err) = self.tmux.send_line(pane, text).await {
            let _ = self.store.cancel_send(pending.id).await;
            return Err(dispatch_err);
        }
        // The send is on its way: mark the turn in flight so a following
        // branch/quoted send defers behind it instead of dispatching mid-turn.
        self.store.set_turn_active(session_id, true).await?;
        Ok(pending)
    }
}
