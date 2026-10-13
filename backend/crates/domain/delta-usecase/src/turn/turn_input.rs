//! Every signal that can move a session's turn state.

#[cfg(doc)]
use super::{OrphanedSend, TurnState};

/// Every signal that can move a session's turn state. Each existing trigger in
/// the system maps onto exactly one of these inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnInput {
    /// Delta dispatched a send: its keystrokes were typed into the pane (or are
    /// held for a resuming pane), and its `UserPromptSubmit` echo is expected.
    /// `slash_command` carries into [`TurnState::AwaitingEcho`]; build it with
    /// [`TurnInput::dispatch`] so it is derived from the send's text.
    Dispatch { send_id: i64, slash_command: bool },
    /// A prompt was submitted, and `send_id` names the send it consumed.
    ///
    /// Which send that is — if any — is decided by POSITION, not by text: the
    /// caller reports `Some(id)` when a send was outstanding *and* its
    /// keystrokes really were in the pane, so this prompt can only be that
    /// send's however Claude Code rewrote it on the way; and `None` when
    /// nothing was outstanding (the prompt was typed straight into the pane)
    /// or the session is inside its resume window, where the outstanding
    /// send's keystrokes are still held and so cannot be what submitted. The
    /// `Option` is computed in `interactor::hooks::on_user_prompt_submit`.
    ///
    /// `None` arriving while a send is outstanding is therefore **not**
    /// anomalous: it is the resume window, a designed-for outcome, and the
    /// same convention [`TurnInput::EchoDeadline`] documents applies.
    PromptSubmitted { send_id: Option<i64> },
    /// A dispatched send was resolved by a client-side slash command rather
    /// than by a model turn: the transcript ingest saw the command's own line
    /// (a known local command's name line, or an "Unknown command: …" notice)
    /// consume send `send_id`, which ends the degenerate turn that send stood
    /// for.
    ///
    /// Only transcripts that record the command produce this input. Claude
    /// Code up to 2.1.285 wrote a three-line group per local command (an
    /// `isMeta` caveat line, the bare command-name line, a stdout line); from
    /// 2.1.286 a local command such as `/cost` leaves no transcript line at
    /// all, and its turn ends on the slash-command echo deadline instead
    /// ([`TurnInput::EchoDeadline`]). This path stays for transcripts written
    /// by older versions, including resumed sessions.
    ///
    /// Such a command fires no `UserPromptSubmit` and no `Stop`, so without a
    /// command line or the deadline nothing would leave
    /// [`TurnState::AwaitingEcho`] and every later send would defer. This is
    /// the honest description of that end — the send was delivered and is
    /// already `matched` by the same fold — so it is
    /// **not** anomalous, orphans nothing, and spends none of the caller's
    /// requeue budget. (Routing it as a plain [`TurnInput::Stop`] instead used
    /// to land on the defensive `(AwaitingEcho, Stop)` arm, logging an anomaly
    /// and claiming a requeue for a row that was already matched.)
    CommandResolved { send_id: i64 },
    /// The `Stop` hook fired: the turn completed.
    Stop,
    /// The `[Request interrupted by user]` marker was ingested from the
    /// transcript: the user aborted the in-flight turn (no `Stop` fires).
    Interrupt,
    /// The session lost its live pane: closed, its launch failed, or it was
    /// reaped. Whatever turn existed can no longer progress.
    Close,
    /// The just-dispatched send's keystrokes never reached the pane.
    DispatchFailed,
    /// An explicit user cancel of the outstanding dispatched send: the browser
    /// asked to abandon a send whose echo has not arrived, so Delta injected
    /// `Escape` into the pane (discarding the composer buffer) and the send is
    /// now cancelled. Exits [`TurnState::AwaitingEcho`] back to
    /// [`TurnState::Idle`] with [`OrphanedSend::Cancel`], so any queued sends
    /// behind it promote naturally on the next idle dispatch.
    Cancel { send_id: i64 },
    /// The echo-deadline watchdog fired for a dispatched send: nothing at all
    /// has been heard about it for longer than the deadline.
    ///
    /// Every other input here is an *event* — a hook, an ingested transcript
    /// line, a browser command. That is exactly why a send whose keystrokes
    /// vanish without a trace (a TUI modal swallowing the paste and eating the
    /// trailing Enter, a human pressing Escape in the attached pane) used to
    /// wedge the queue forever: no event ever arrives, so no transition ever
    /// runs and [`TurnState::AwaitingEcho`] is never left. This input turns
    /// "no signal at all" into a signal, making the absence itself observable
    /// after a bounded wait.
    ///
    /// It is deliberately **not** anomalous in any state: firing on the
    /// outstanding send is the designed-for outcome, and firing late (the echo
    /// settled while the sweep was in flight, or a `Stop`/`Cancel` beat it) is
    /// an ordinary race whose stale no-op needs no warning.
    ///
    /// For a **slash-command** send the silence means the opposite: Claude
    /// Code ran a local command (`/cost`, `/model`, …), which fires no hook
    /// and, from 2.1.286, writes no transcript line either. The deadline then
    /// settles the send as delivered instead of requeueing it — re-typing
    /// would run the command a second time — and the runtime measures it
    /// against a much shorter wait
    /// ([`SLASH_COMMAND_ECHO_DEADLINE`](crate::SLASH_COMMAND_ECHO_DEADLINE)).
    /// A slash command that is really a prompt (a skill, a custom command)
    /// echoes within seconds and so never reaches it. If its
    /// `UserPromptSubmit` does arrive after the settle, the session is already
    /// [`TurnState::Idle`], and the ordinary `(Idle, PromptSubmitted)` arm
    /// tracks it as pane input.
    EchoDeadline { send_id: i64 },
}

impl TurnInput {
    /// The [`TurnInput::Dispatch`] for a send whose text is `text`, with
    /// `slash_command` derived by the same predicate the transcript fold uses
    /// to let a command line consume a send.
    pub fn dispatch(send_id: i64, text: &str) -> Self {
        Self::Dispatch {
            send_id,
            slash_command: delta_attribution::claude_format::is_slash_command_send(text),
        }
    }
}
