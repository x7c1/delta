use delta_model::{MessageUuid, Send, ThreadId};

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
    /// Write the `send` row and dispatch the keystrokes for an open
    /// session, with the cancel-on-dispatch-failure rollback.
    ///
    /// Returns the created send plus any [`SessionEvent`]s the enqueue
    /// produced (the idle-flush below may promote a previously-queued send,
    /// which the caller must broadcast as `send_dispatched`).
    pub(in crate::interactor::enqueue) async fn enqueue_into_open(
        &mut self,
        pane: &str,
        thread_id: ThreadId,
        text: &str,
        locator_quote: Option<&str>,
        branch_from: Option<&MessageUuid>,
    ) -> Result<(Send, Vec<SessionEvent>)> {
        let mut events = Vec::new();

        // Idle-flush safety net: if the turn is idle but a send is still
        // `queued` (a dispatch trigger was missed, e.g. an interrupt the tail
        // had not tailed yet), flush it now so it keeps its place ahead of
        // this new send in FIFO order. Dispatching it moves the turn machine
        // to `AwaitingEcho`, which the defer check below then observes. While
        // the session is still inside its resume-readiness window this flush
        // is a no-op (typing into the not-yet-input-ready pane would lose the
        // keystrokes); the queued row is flushed at resume settle instead.
        // Held rows — restored at boot or parked by the echo deadline — are
        // excluded by the queued selection: they wait for an explicit
        // release, never for this flush.
        if self.state.turn() == TurnState::Idle {
            if let Some(event) = self.dispatch_queued_send().await? {
                events.push(event);
            }
        }

        // The target thread was already loaded by the routing layer to derive
        // the session, so its existence is established here (a stale/wrong id
        // surfaced as `ThreadNotFound` before reaching this point). Branch
        // bookkeeping (the new thread lane + semantic parent) is shared with the
        // Codex adapter path via `resolve_branch_target`.
        let (target_thread, semantic_parent) = self
            .resolve_branch_target(thread_id, branch_from, locator_quote)
            .await?;

        // Defer this send when the turn is not idle (single-outstanding
        // dispatch): only one send may be out per turn, so anything composed
        // while a dispatch is outstanding or a turn is running is held
        // `queued` (the branch child thread and the held text persist) and
        // dispatched by the turn-end triggers, one at a time. This also keeps
        // every quoted/branch send out of Claude Code's own mid-turn queue,
        // where its `UserPromptSubmit` hook — and therefore its locator quote
        // — would be lost.
        if self.state.turn() != TurnState::Idle {
            let send = self
                .store
                .enqueue_queued_send(
                    self.id,
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
                self.id,
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
        // would lose it, so hold this prompt's keystroke on the runtime state
        // instead and let the resume tick type it once `SessionStart(resume)`
        // has marked the resume ready and it has settled. The `send` row above
        // is already written (its thread/branch/quote semantics persisted), so
        // only the physical keystroke is held. The turn machine still moves to
        // `AwaitingEcho`, so anything composed before readiness defers behind
        // this first prompt rather than racing it into the pane. A freshly
        // resumed session has an idle turn, so this readiness gate is the only
        // gate in play until the first prompt dispatches; once ready
        // (the resuming entry gone), later sends fall through to the immediate
        // dispatch below.
        if self.state.hold_first_prompt(text.to_owned()) {
            tracing::info!(
                session_id = %self.id,
                "send held until resume readiness (SessionStart=resume); keystroke held"
            );
            self.apply_turn_input(TurnInput::Dispatch { send_id: send.id })
                .await?;
            return Ok((send, events));
        }

        // The send is on its way: move the turn machine to `AwaitingEcho`
        // BEFORE typing, so the `UserPromptSubmit` echo (which can fire within
        // milliseconds of the keystrokes landing) always finds the dispatch
        // recorded, and a following send defers behind it instead of
        // dispatching mid-turn.
        self.apply_turn_input(TurnInput::Dispatch { send_id: send.id })
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
            let _ = self.apply_turn_input(TurnInput::DispatchFailed).await;
            return Err(dispatch_err);
        }
        Ok((send, events))
    }
}
