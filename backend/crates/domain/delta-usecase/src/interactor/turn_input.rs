//! Feeding signals into the per-session turn state machine.
//!
//! The transitions themselves live in the `turn` module (one exhaustive
//! table); this file is the single place that *executes* a transition's side
//! effects — the orphaned-send disposition and the anomaly logging — so every
//! call site feeds the machine the same way. It runs inside the session's
//! actor, where the turn state is plain owned data: the mailbox already
//! serialized every input that can move it.
//!
//! The table is a pure function of (state, input), so anything that depends on
//! *history* lives here instead. The requeue budget is the one such rule: the
//! table says "requeue this orphaned send", and this file decides whether that
//! send has already had its retry — parking it if it has, so a send nothing is
//! ever heard about stops re-dispatching instead of looping forever. Every
//! producer of [`OrphanedSend::Requeue`] shares that budget, and after prompt
//! consumption became positional the echo-deadline watchdog
//! ([`crate::interactor::echo_deadline`]) is essentially its only live one: a
//! prompt that arrives consumes the outstanding send whatever its text says,
//! so only genuine silence still requeues.
//!
//! The other history-dependent rule lives here for the same reason: a requeue
//! fired inside the **resume window** must also drop the resume's held copy of
//! the keystrokes ([`SessionRuntime::drop_held_prompt`]). The table cannot know
//! that copy exists, and leaving it would have the settle type the message and
//! the next idle flush dispatch the requeued row — the same message delivered
//! twice.
//!
//! [`SessionRuntime::drop_held_prompt`]: crate::interactor::session_actor::runtime::SessionRuntime::drop_held_prompt

use crate::agent::{AgentEvent, TurnStatus};
use crate::error::Result;
use crate::interactor::session_actor::actor::SessionContext;
use crate::ports::{GitWorktree, SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace};
use crate::turn::{turn_input_for_agent_event, OrphanedSend, TurnInput, TurnState};

/// What the [`OrphanedSend::Requeue`] disposition actually did with the send,
/// once the budget had its say.
///
/// The table only ever asks for a requeue; whether the send got one is history,
/// and history lives here. Reported to the callers that must branch on the
/// answer — the echo-deadline sweep clears the pane with an `Escape` only when
/// the send really is back in the queue about to be re-typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::interactor) enum RequeueOutcome {
    /// The send is `queued` again and re-dispatches on the next idle flush.
    Requeued,
    /// The budget was spent, so the send was parked instead: its row is back
    /// in the queue holding for an explicit release, and
    /// [`SessionEvent::SendParked`] announced why.
    Parked,
}

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
        Ok(self.apply_turn_input_reporting(input).await?.0)
    }

    /// [`Self::apply_turn_input`], additionally reporting what became of an
    /// orphaned send the table asked to requeue (`None` when the transition
    /// orphaned nothing, or orphaned it with a different disposition).
    ///
    /// Only the echo-deadline sweep needs the answer — it injects an `Escape`
    /// into the pane before the flush re-types, and only a genuinely requeued
    /// send is the one about to be re-typed — so every other call site keeps
    /// the plain signature above.
    pub(in crate::interactor) async fn apply_turn_input_reporting(
        &mut self,
        input: TurnInput,
    ) -> Result<(TurnState, Option<RequeueOutcome>)> {
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

        let mut requeue_outcome = None;
        match result.orphaned {
            None => {}
            Some(OrphanedSend::Requeue(send_id)) => {
                // Inside the resume window this send's keystrokes were never
                // typed: they are still held on the resuming entry, waiting
                // for the settle. The queue is now the message's single owner
                // — whether it is requeued or parked below — so drop the held
                // copy. Outside the window there is no entry to hold anything,
                // and this is a no-op.
                if self.state.drop_held_prompt() {
                    tracing::info!(
                        session_id = %id,
                        send_id,
                        "the requeued send was still held for the resume window; dropping the \
                         held keystrokes so it is typed once, off the queue, and not again at \
                         resume settle"
                    );
                }
                // Requeueing assumes the next dispatch echoes cleanly; the
                // budget is what keeps that optimism from becoming an
                // unbounded loop. See `SessionRuntime::claim_requeue`.
                if self.state.claim_requeue(send_id) {
                    tracing::warn!(
                        session_id = %id,
                        send_id,
                        "outstanding send never echoed; returning it to `queued` so it \
                         re-dispatches when the session is next idle"
                    );
                    self.store.requeue_send(send_id).await?;
                    requeue_outcome = Some(RequeueOutcome::Requeued);
                } else {
                    self.park_unechoable_send(send_id).await?;
                    requeue_outcome = Some(RequeueOutcome::Parked);
                }
            }
            Some(OrphanedSend::Cancel(send_id)) => {
                tracing::warn!(
                    session_id = %id,
                    send_id,
                    "outstanding send can no longer be delivered; cancelling it"
                );
                self.state.forget_requeues(send_id);
                self.store.cancel_send(send_id).await?;
            }
            Some(OrphanedSend::SettleIfUnmatched(send_id)) => {
                // A transcript line normally claimed the send during the turn
                // (leaving `matched`), which makes the guarded update a silent
                // no-op — a rewritten echo claims it just the same, since
                // attribution is positional. It only bites when no human line
                // was ingested at all before the turn ended: the turn stopped
                // early, or a `/compact` swallowed the echo. The message still
                // went out (a prompt submission is what consumed the send to
                // reach `InFlight` at all) and its turn has now ended, so the
                // row settles as *delivered* with no uuid: cancelling would
                // report a delivered message as failed, and leaving it
                // `dispatched` would shadow the next dispatch's correlation.
                let settled = self.store.settle_send_delivered(send_id).await?;
                if settled {
                    tracing::info!(
                        session_id = %id,
                        send_id,
                        "turn ended with its send unattributed: no transcript user line was \
                         ingested for it before the turn ended, so the send settles as \
                         delivered without a matched uuid"
                    );
                    self.state.forget_requeues(send_id);
                }
            }
        }
        Ok((result.next, requeue_outcome))
    }

    /// Park a send that has spent its requeue budget: hold the row in the
    /// queue for an explicit release and announce why, instead of returning it
    /// to `queued` for another doomed dispatch.
    ///
    /// The row goes back to `queued` with the hold marker
    /// ([`SessionStore::hold_send_for_release`]) — the same state the boot
    /// restore leaves an orphaned row in. It therefore stays in the open-send
    /// list, is skipped by every automatic dispatch trigger, and waits for the
    /// user to release it (re-typed once) or cancel it. Cancelling instead
    /// would leave the composed text alive only inside the broadcast event, so
    /// a reload, a second tab, or a session switch lost the message for good —
    /// the exact silent loss this path exists to prevent.
    ///
    /// The hold is still paired with a [`SessionEvent::SendParked`], because
    /// the queue row alone says "waiting" and not *why*: the notice is how the
    /// user learns their message was swallowed rather than merely slow. The
    /// event keeps carrying the text — it saves a client a refetch — but the
    /// row, not the event, is where the message now lives: Delta's own browser
    /// renders the row and ignores this copy.
    ///
    /// The event goes out on the async seam because this runs
    /// deep inside a transition whose callers have no event channel; the
    /// server drains it onto the same broadcast the synchronous paths feed.
    ///
    /// The row is read back before the hold so the event can carry its text.
    /// If the guarded hold reports no transition the send is no longer
    /// `dispatched` — it matched, settled, or was cancelled while the deadline
    /// sweep ran — and there is nothing to hold and nothing to announce.
    async fn park_unechoable_send(&mut self, send_id: i64) -> Result<()> {
        let parked = self.store.send(send_id).await?;
        self.state.forget_requeues(send_id);
        if !self.store.hold_send_for_release(send_id).await? {
            tracing::info!(
                session_id = %self.id,
                send_id,
                "the send whose echo deadline expired twice is no longer dispatched, so \
                 there is nothing to park: it matched, settled, or was cancelled while \
                 the sweep ran"
            );
            return Ok(());
        }
        tracing::warn!(
            session_id = %self.id,
            send_id,
            "outstanding send never echoed again after its one re-dispatch; its echo \
             appears unmatchable, so it is parked — held in the queue until the user \
             releases or cancels it — instead of requeued"
        );
        self.emit_async_event(SessionEvent::SendParked {
            session_id: self.id.clone(),
            send_id,
            // `None` is unreachable once the guarded hold reports a
            // transition: it only ever updates a row it found. Defaulting
            // keeps the notice honest instead of panicking on an invariant.
            text: parked.map(|send| send.text).unwrap_or_default(),
        });
        Ok(())
    }

    /// Apply a turn-*end* fact, expressed as a provider-neutral
    /// [`AgentEvent::TurnCompleted`] status, to the session's turn machine.
    ///
    /// Every live turn-end path that ends a turn *generically* — the `Stop`
    /// hook, the transcript interrupt marker, the API-error abort — names its
    /// end as a [`TurnStatus`] and routes through here, so
    /// [`turn_input_for_agent_event`] — the single neutral mapping proven in
    /// the `turn` module — owns which [`TurnInput`] a completed / interrupted
    /// / failed turn feeds the machine, rather than each call site choosing
    /// `Stop`/`Interrupt` directly. The mapping is total for a `TurnCompleted`
    /// event, so the `expect` documents an invariant rather than hiding a
    /// fallible parse.
    pub(in crate::interactor) async fn apply_turn_end(
        &mut self,
        status: TurnStatus,
    ) -> Result<TurnState> {
        let input = turn_input_for_agent_event(&AgentEvent::TurnCompleted { status })
            .expect("a TurnCompleted event always maps to a turn-end input");
        self.apply_turn_input(input).await
    }
}
