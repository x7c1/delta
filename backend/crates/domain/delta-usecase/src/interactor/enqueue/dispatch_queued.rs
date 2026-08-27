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
    /// is recorded, and the session has something live to dispatch into — a
    /// bound pane (Claude) or an open adapter (Codex).
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
    /// **Both providers.** A pane-backed session types the promoted row into
    /// its pane; a terminal-less (adapter-backed) session starts a turn on its
    /// adapter instead, exactly as [`Self::dispatch_agent_turn`] does for a
    /// send that arrives while the session is already idle. The two share this
    /// one selection step so the FIFO, the held-row skip and the
    /// single-outstanding rule cannot drift apart per provider.
    ///
    /// A no-op (returning `None`) when the turn is not idle, the session is
    /// still inside its resume-readiness window, there is no queued send, or
    /// the session has neither a live pane nor an open adapter (closed) — in
    /// which case the send stays `queued` and is dispatched by the next trigger
    /// that reaches this method: a turn end (Claude's `Stop` hook or an
    /// adapter's `turn/completed`), an interrupt ingest, a resume settle
    /// (`dispatch_ready_resume`), a dispatched-send cancellation, a
    /// held-send release (`release_send`), `enqueue_into_open`'s idle-flush, or
    /// the flush a fresh spawn runs when it binds
    /// ([`Self::flush_queued_send_async`]). *Held* rows — those carrying
    /// `held_at`, whether the boot restore recovered them from a dead process's
    /// `dispatched` state or the echo deadline parked them — are invisible to
    /// this method entirely:
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

        // A terminal-less session has no pane to type into: promote the row and
        // start its turn on the adapter, the way every other adapter-backed
        // send goes out. `start_agent_turn` owns the rest (turn machine, row
        // completion at the `turn/start` ack, cancellation on failure), so the
        // queued path and the immediate path cannot diverge.
        if let Some(agent) = self.state.open_agent() {
            let adapter = agent.adapter.clone();
            let handle = agent.handle.clone();
            self.store.promote_queued_send(send.id).await?;
            // A branch row carries the passage it replies to; deliver it as
            // hidden context before the turn starts, exactly as a branch send
            // dispatched straight from `enqueue_to_thread` does. A plain row
            // (every row a still-spawning session accepts) carries neither.
            if send.semantic_parent_uuid.is_some() {
                if let Some(quote) = send.locator_quote.as_deref() {
                    adapter.inject_context(&handle, quote).await?;
                }
            }
            self.start_agent_turn(
                &adapter,
                &handle,
                send.id,
                send.thread_id,
                send.semantic_parent_uuid.clone(),
                send.text,
            )
            .await?;
            return Ok(Some(SessionEvent::SendDispatched {
                session_id: self.id.clone(),
                send_id: send.id,
            }));
        }

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

    /// Run [`Self::dispatch_queued_send`] on a path with no caller to return to,
    /// announcing any promotion on the async event seam.
    ///
    /// Three triggers reach the queue outside a request: the flush posted to
    /// this actor when a fresh Claude spawn binds (a hook must not type
    /// keystrokes itself, so it posts instead — see `bind_pending_spawn`), the
    /// same flush at the end of an adapter-backed bind (called inline there —
    /// see `activate_adapter_session`), and an adapter-backed session's turn
    /// end, which arrives on the event pump. None has a `Vec<SessionEvent>` to
    /// fold into, and none may fail the caller: a dispatch error is logged and
    /// the row stays `queued` for the next trigger.
    pub(in crate::interactor) async fn flush_queued_send_async(&mut self) {
        match self.dispatch_queued_send().await {
            Ok(Some(event)) => self.emit_async_event(event),
            Ok(None) => {}
            Err(err) => tracing::error!(
                session_id = %self.id,
                error = %err,
                "failed to dispatch a queued send; it stays queued for the next flush"
            ),
        }
    }
}
