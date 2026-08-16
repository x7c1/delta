//! The echo-deadline watchdog: the last-resort recovery for a dispatched send
//! whose keystrokes vanish without leaving any trace at all.
//!
//! Every other recovery Delta has is event-driven — a mismatched echo, a turn
//! end, a compact summary, a browser cancel — and each needs *something* to
//! arrive before it can act. The failure this file exists for produces nothing
//! to arrive: Claude Code's TUI puts up an interactive modal between turns, the
//! pasted text is swallowed whole (it appears in no scrollback, composer, or
//! transcript) and the trailing Enter answers the modal instead of submitting a
//! prompt. No user message, no `UserPromptSubmit`, no turn boundary. The turn
//! machine sits in [`TurnState::AwaitingEcho`] forever, the row stays
//! `dispatched`, and everything queued behind it waits for a turn that will
//! never end.
//!
//! So the absence itself is made observable: a send that has been awaited for
//! longer than [`LaunchConfig::echo_deadline`] is fed
//! [`TurnInput::EchoDeadline`], and the recovery from there rides machinery
//! that already exists — the requeue budget in
//! [`turn_input`](crate::interactor::turn_input). One deadline returns the send
//! to `queued` and re-types it; a second parks it (row `cancelled`, text handed
//! back through [`SessionEvent::SendParked`]) and the queue behind it drains.
//! Two dispatches are enough to tell "the modal was up for a moment" from "the
//! keystrokes can never land".
//!
//! Nothing here is specific to the modal that exposed it: whatever swallows a
//! keystroke silently — a human pressing `Escape` in the attached pane, a TUI
//! state Delta has never seen — is caught by the same net, because the signal
//! is the silence.
//!
//! [`LaunchConfig::echo_deadline`]: crate::launch_config::LaunchConfig::echo_deadline

use std::time::Instant;

use crate::error::Result;
use crate::interactor::question_keys::cancel_keys;
use crate::interactor::session_actor::actor::SessionContext;
use crate::interactor::turn_input::RequeueOutcome;
use crate::ports::{GitWorktree, SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace};
use crate::turn::TurnInput;

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Give up on this session's outstanding send if nothing at all has been
    /// heard about it before its deadline, and flush the queue behind it.
    ///
    /// A no-op — the overwhelming common case — unless a send is genuinely
    /// being awaited and its wait has run past the deadline (see
    /// [`SessionRuntime::expired_echo_deadline`], which also holds the sweep
    /// off while a resume's first prompt is still deliberately held).
    ///
    /// When the deadline does fire:
    ///
    /// 1. [`TurnInput::EchoDeadline`] returns the machine to
    ///    [`TurnState::Idle`] and orphans the send as a requeue, which the
    ///    budget turns into either a retry or a park.
    /// 2. On a retry, a single `Escape` is injected into the pane before the
    ///    re-type — the same primitive the dispatched-send cancel uses. The
    ///    deadline means *something* is holding the keystrokes; `Escape`
    ///    dismisses a lingering modal and discards a partially-landed composer
    ///    draft, so the re-type stays idempotent even in the "text landed but
    ///    Enter was eaten" variant. Nothing else re-types with a leading
    ///    `Escape`: the normal dispatch and the compact re-dispatch have no
    ///    reason to suspect the pane's state.
    /// 3. The queued-send flush runs in the same actor turn, so the requeued
    ///    send re-types immediately instead of waiting for the next unrelated
    ///    idle signal — and after a park, the send queued behind it dispatches
    ///    there and then. The flush picks the session's oldest `queued` row,
    ///    which after a requeue is necessarily the requeued send itself: it was
    ///    the outstanding one, so every other open send was composed after it
    ///    and carries a higher id.
    ///
    /// `now` is injected (rather than read here) so the sweep is deterministic
    /// under test, exactly like the launch watchdog's reap: the server loop
    /// passes `Instant::now()`, tests advance a synthetic clock. Returns the
    /// [`SessionEvent::SendDispatched`] the flush produced, if any, for the
    /// caller to broadcast.
    ///
    /// [`SessionRuntime::expired_echo_deadline`]: crate::interactor::session_actor::runtime::SessionRuntime::expired_echo_deadline
    /// [`TurnState::AwaitingEcho`]: crate::turn::TurnState::AwaitingEcho
    /// [`TurnState::Idle`]: crate::turn::TurnState::Idle
    pub(in crate::interactor) async fn sweep_echo_deadline(
        &mut self,
        now: Instant,
    ) -> Result<Option<SessionEvent>> {
        let Some(send_id) = self
            .state
            .expired_echo_deadline(now, self.launch.echo_deadline)
        else {
            return Ok(None);
        };
        tracing::warn!(
            session_id = %self.id,
            send_id,
            deadline_ms = self.launch.echo_deadline.as_millis(),
            "dispatched send produced no echo and no other signal before its deadline; \
             its keystrokes were swallowed (a TUI dialog, or an Escape in the pane), so \
             the turn is released and the send retried once before being parked"
        );

        let (_, requeued) = self
            .apply_turn_input_reporting(TurnInput::EchoDeadline { send_id })
            .await?;

        if requeued == Some(RequeueOutcome::Requeued) {
            // Clear whatever is holding the pane before the flush re-types the
            // send into it. Best-effort by design: a pane that cannot take the
            // Escape cannot take the re-type either, and the following flush
            // surfaces that failure through its own cancel-on-dispatch-failure
            // path — so a failed key injection must not abort the recovery.
            self.escape_pane_before_retype().await;
        }

        // Flush now rather than waiting for an unrelated idle signal: after a
        // retry this is the re-type itself, and after a park it promotes
        // whatever was stuck behind the parked head.
        //
        // A dispatch failure is logged rather than propagated, mirroring the
        // resume tick: `dispatch_queued_send` already cancelled the row it
        // failed on, so propagating buys no recovery — it would only abort the
        // sweep and drop the `SendDispatched` broadcasts the *other* sessions'
        // flushes produced in this same tick, leaving their browsers showing a
        // send as still queued. Whatever swallows keystrokes tends to hit
        // several panes at once, so those siblings are exactly the sends this
        // tick just rescued.
        match self.dispatch_queued_send().await {
            Ok(event) => Ok(event),
            Err(err) => {
                tracing::warn!(
                    session_id = %self.id,
                    error = %err,
                    "failed to re-type or promote a send after its echo deadline"
                );
                Ok(None)
            }
        }
    }

    /// Inject a single `Escape` into the session's pane, logging (but
    /// swallowing) a failure. Nothing happens when the session has no live
    /// pane — there is no composer state to discard.
    async fn escape_pane_before_retype(&mut self) {
        let Some(pane) = self.state.handle().map(|handle| handle.pane.clone()) else {
            return;
        };
        let keys = cancel_keys();
        let names: Vec<&str> = keys.iter().map(|key| key.tmux_name()).collect();
        if let Err(err) = self.tmux.send_keys(&pane, &names).await {
            tracing::warn!(
                session_id = %self.id,
                error = %err,
                "failed to clear the pane before re-typing a deadline-requeued send \
                 (continuing: the re-type reports its own failure)"
            );
        }
    }
}
