//! Stopping a Claude session's in-flight turn from the browser.
//!
//! The browser's Stop control does what pressing `Escape` in the pane does —
//! Delta injects that keystroke, then `Ctrl+C` to empty the composer — and
//! then ends the turn on Delta's side at once, without waiting for a signal
//! Claude Code may never send.
//!
//! Waiting is not an option because of the case this exists for: the user
//! presses Escape in the pane after the prompt submitted (so
//! `UserPromptSubmit` moved the turn machine to [`TurnState::InFlight`]) but
//! before any assistant output streamed. Claude then drops the prompt, puts
//! its text back into the composer, and writes nothing more — no assistant line and no `[Request
//! interrupted by user]` marker, so the transcript interrupt path never runs —
//! and fires no `Stop` hook (it never does on a user interrupt, and
//! `StopFailure` covers API errors only). The turn machine would stay
//! `InFlight` forever and every later send would wait `queued` behind a turn
//! that no longer exists. No reliable automatic signal exists (scraping the
//! TUI was rejected as too brittle), so the user's Stop is the signal.
//!
//! One control covers both cases with the same meaning: when Claude is really
//! working, the Escape interrupts it; when Delta has merely lost track of a
//! turn Claude already abandoned, the keys land on an idle composer and the
//! strip recovers. Either way the `Ctrl+C` leaves the composer empty for the
//! queued send typed next: Claude Code does not clear the prompt it restored,
//! and that send would otherwise be spliced into it.
//!
//! The turn ends exactly as an ingested interrupt marker ends it — a
//! [`TurnStatus::Interrupted`] turn end, i.e. [`TurnInput::Interrupt`] from
//! `InFlight` back to `Idle` with its `SettleIfUnmatched` disposition for the
//! turn's send — and [`SessionEvent::TurnInterrupted`] is announced for the
//! turn's thread. When Claude was streaming it does write the marker for the
//! injected Escape, a moment later; that marker is recognised as this stop's
//! and does not end a second turn (see
//! `SessionRuntime::expect_stopped_turn_marker`).
//!
//! Refused with [`Error::TurnNotStoppable`] whenever there is no such turn to
//! stop, before any keystroke is sent:
//!
//! - a terminal-less (Codex) session — it is stopped through its provider's
//!   own [`interrupt`](super::interrupt);
//! - a session with no live pane — closed, ended or still starting, nothing is
//!   running in it;
//! - [`TurnState::Idle`] — nothing is running;
//! - [`TurnState::AwaitingEcho`] — the dispatched send's own cancel covers that
//!   state (it injects the same Escape), and stopping here would race the
//!   echo.
//!
//! Runs inside the session's actor, so the stop is ordered against the
//! session's hooks, its transcript tail and its dispatch path.
//!
//! [`TurnInput::Interrupt`]: crate::turn::TurnInput::Interrupt
//! [`TurnState::AwaitingEcho`]: crate::turn::TurnState::AwaitingEcho
//! [`TurnState::Idle`]: crate::turn::TurnState::Idle
//! [`TurnState::InFlight`]: crate::turn::TurnState::InFlight

use crate::agent::TurnStatus;
use crate::error::{Error, Result};
use crate::interactor::session_actor::actor::SessionContext;
use crate::ports::{GitWorktree, SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace};
use crate::turn::TurnState;
use crate::turn_stop_refusal::TurnStopRefusal;

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Stop this Claude session's in-flight turn: press `Escape` and then
    /// `Ctrl+C` in its pane, then end the turn on Delta's side.
    ///
    /// Returns the events to broadcast, in order: the
    /// [`SessionEvent::TurnInterrupted`] for the stopped turn, then the
    /// [`SessionEvent::SendDispatched`] when a queued send went out on the
    /// now-idle session. Refused with [`Error::TurnNotStoppable`] — touching
    /// nothing — when there is no in-flight turn to stop (see the module docs
    /// for each refusal). A failed key injection is returned as is, with the
    /// turn left `InFlight`: Claude may not have been stopped, so the turn is
    /// not reported as stopped.
    ///
    /// The next queued send is dispatched at once, exactly as the transcript
    /// tail does after ingesting an interrupt marker. It lands in an empty
    /// composer because of the `Ctrl+C` (see
    /// [`press_escape_and_clear_composer`](Self::press_escape_and_clear_composer)).
    pub(in crate::interactor) async fn stop_turn(&mut self) -> Result<Vec<SessionEvent>> {
        let pane = self.stoppable_turn_pane()?;
        self.press_escape_and_clear_composer(&pane).await?;

        // Resolve the turn's thread BEFORE the machine runs: the transition
        // can settle the head dispatched send, which is the authoritative
        // thread source — the same order `on_stop` and the interrupt marker
        // path use.
        let thread_id = self.store.in_progress_turn_thread(self.id).await?;
        let from = self.state.turn();
        self.apply_turn_end(TurnStatus::Interrupted).await?;
        self.state.expect_stopped_turn_marker();
        tracing::info!(
            session_id = %self.id,
            from = ?from,
            thread_id = %thread_id,
            "turn stopped by the user: Escape and Ctrl+C sent to the pane and the \
             turn ended without waiting for an interrupt marker"
        );

        let mut events = vec![SessionEvent::TurnInterrupted {
            session_id: self.id.clone(),
            thread_id: Some(thread_id),
        }];
        if let Some(event) = self.dispatch_queued_send().await? {
            events.push(event);
        }
        Ok(events)
    }

    /// The pane of this session's stoppable in-flight turn, or the refusal
    /// naming why there is none.
    fn stoppable_turn_pane(&self) -> Result<String> {
        let refusal = if self.state.open_agent().is_some() {
            TurnStopRefusal::NoTerminal
        } else if let Some(handle) = self.state.handle() {
            match self.state.turn() {
                TurnState::InFlight { .. } => return Ok(handle.pane.clone()),
                TurnState::Idle => TurnStopRefusal::Idle,
                TurnState::AwaitingEcho { .. } => TurnStopRefusal::AwaitingEcho,
            }
        } else {
            TurnStopRefusal::NotOpen
        };
        Err(Error::TurnNotStoppable {
            session_id: self.id.as_str().to_owned(),
            reason: refusal,
        })
    }
}
