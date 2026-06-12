use delta_model::{MessageUuid, Send, SessionId, ThreadId};

use crate::error::Result;
use crate::ports::{SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace};
use crate::turn::{TurnInput, TurnState};
use crate::interactor::InteractorCore;

use super::provisional_branch_title;

impl<T, X, S, W> InteractorCore<T, X, S, W>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
{
    /// Write the `send` row and dispatch the keystrokes for an open
    /// session, with the cancel-on-dispatch-failure rollback.
    ///
    /// Returns the created send plus any [`SessionEvent`]s the enqueue
    /// produced (the idle-flush below may promote a previously-queued send,
    /// which the caller must broadcast as `send_dispatched`).
    #[allow(clippy::too_many_arguments)]
    pub(in crate::interactor::enqueue) async fn enqueue_into_open(
        &self,
        session_id: &SessionId,
        pane: &str,
        thread_id: ThreadId,
        text: &str,
        locator_quote: Option<&str>,
        branch_from: Option<&MessageUuid>,
    ) -> Result<(Send, Vec<SessionEvent>)> {
        let mut events = Vec::new();

        // Idle-flush safety net: if the turn is idle but a send is still
        // `queued` (a dispatch trigger was missed, e.g. an interrupt the tail
        // had not tailed yet), release it now so it keeps its place ahead of
        // this new send in FIFO order. Dispatching it moves the turn machine
        // to `AwaitingEcho`, which the defer check below then observes.
        if self.turn_state_for(session_id).await == TurnState::Idle {
            if let Some(event) = self.dispatch_queued_send(session_id).await? {
                events.push(event);
            }
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
                    .create_thread(session_id, &title, Some(thread_id))
                    .await?;
                (thread.id, Some(parent.clone()))
            }
            None => (thread_id, None),
        };

        // Defer this send when the turn is not idle (single-outstanding
        // dispatch): only one send may be out per turn, so anything composed
        // while a dispatch is outstanding or a turn is running is held
        // `queued` (the branch child thread and the held text persist) and
        // dispatched by the turn-end triggers, one at a time. This also keeps
        // every quoted/branch send out of Claude Code's own mid-turn queue,
        // where its `UserPromptSubmit` hook — and therefore its locator quote
        // — would be lost.
        if self.turn_state_for(session_id).await != TurnState::Idle {
            let send = self
                .store
                .enqueue_queued_send(
                    session_id,
                    target_thread,
                    semantic_parent.as_ref(),
                    text,
                    locator_quote,
                )
                .await?;
            return Ok((send, events));
        }

        let send = self
            .store
            .enqueue_send(
                session_id,
                target_thread,
                semantic_parent.as_ref(),
                text,
                locator_quote,
            )
            .await?;

        // Resume readiness gate: a session resumed by `claude --resume <id>` binds
        // its pane immediately but is not ready to accept input until its
        // `SessionStart(source=resume)` arrives (~2s later — far past any safe
        // fixed settle). Dispatching the first keystroke into that still-cold pane
        // would lose it, so hold this prompt's keystroke on the registry instead
        // and let `dispatch_ready_resumes` type it on the background tick once
        // `SessionStart(resume)` has marked the resume ready and it has settled.
        // The `send` row above is already written (its thread/branch/quote
        // semantics persisted), so only the physical keystroke is held. The
        // turn machine still moves to `AwaitingEcho`, so anything composed
        // before readiness defers behind this first prompt rather than racing
        // it into the pane. A freshly resumed session has an idle turn, so
        // this readiness gate is the only gate in play until the first prompt
        // dispatches; once ready (membership gone), later sends fall through
        // to the immediate dispatch below.
        if self
            .open_sessions
            .lock()
            .await
            .hold_first_prompt(session_id, text.to_owned())
        {
            tracing::info!(
                session_id = %session_id,
                "send held until resume readiness (SessionStart=resume); keystroke held"
            );
            self.apply_turn_input(session_id, TurnInput::Dispatch { send_id: send.id })
                .await?;
            return Ok((send, events));
        }

        // The send is on its way: move the turn machine to `AwaitingEcho`
        // BEFORE typing, so the `UserPromptSubmit` echo (which can fire within
        // milliseconds of the keystrokes landing) always finds the dispatch
        // recorded, and a following send defers behind it instead of
        // dispatching mid-turn.
        self.apply_turn_input(session_id, TurnInput::Dispatch { send_id: send.id })
            .await?;

        // If the keystrokes never reach the pane, the row we just wrote would
        // sit outstanding forever and shadow all future `UserPromptSubmit`
        // correlation. The `DispatchFailed` transition cancels it so the
        // outstanding slot clears, then the original dispatch error surfaces.
        // We do *not* roll back the just-created branch child thread: an
        // empty, unnamed thread is harmless overlay data and may legitimately
        // be reused by a retry, whereas the correlation-shadowing dispatched
        // row is the actual hazard this guard exists to clear.
        if let Err(dispatch_err) = self.tmux.send_line(pane, text).await {
            // Best-effort: if the cleanup itself fails we keep the dispatch
            // error (the caller's actionable failure) rather than masking it
            // with a store error.
            let _ = self
                .apply_turn_input(session_id, TurnInput::DispatchFailed)
                .await;
            return Err(dispatch_err);
        }
        Ok((send, events))
    }
}
