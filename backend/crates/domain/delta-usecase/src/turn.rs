//! The per-session turn state machine.
//!
//! A *turn* is one prompt→response round trip in a Claude Code session. Delta
//! used to track it as a persisted boolean (`session.turn_active`) mutated from
//! several scattered call sites; this module replaces that with one explicit
//! state machine whose every transition is defined — and unit-tested — in a
//! single place ([`transition`]).
//!
//! ## States
//!
//! - [`TurnState::Idle`] — no turn in flight. The only state a queued send may
//!   be dispatched from (the *single-outstanding* rule: at most one
//!   `dispatched` send exists per session, so a `UserPromptSubmit` arriving
//!   while a send is outstanding is *that* send's prompt — a positional fact,
//!   needing neither a FIFO scan nor a text comparison).
//! - [`TurnState::AwaitingEcho`] — Delta dispatched a send (its keystrokes were
//!   typed, or are held for a resuming pane) and is waiting for the
//!   `UserPromptSubmit` hook to echo it back.
//! - [`TurnState::InFlight`] — a turn is running: either the echoed Delta send
//!   (`send_id: Some`) or a prompt typed straight into the pane
//!   (`send_id: None`).
//!
//! ## Position decides consumption and attribution; text only flags a rewrite
//!
//! A Claude Code session gives Delta no round-trippable id for a prompt it
//! typed into the pane: `UserPromptSubmit` carries only the prompt text. Two
//! separate questions used to hang off one text comparison — *has the
//! dispatched send's turn started?* and *which thread do this turn's
//! transcript lines belong to?* — so every rewrite Claude Code applies between
//! typing and recording (local-command folding, namespace expansion, the
//! `[Image #N]` prefix) made the machine re-type a message that had already
//! been delivered, and filed the turn's lines under the wrong thread.
//!
//! Both are now answered by position. **Consumption is positional**: while a
//! send is outstanding and its keystrokes really are in the pane (not held for
//! a resuming session), the next `UserPromptSubmit` is that send's — whatever
//! it says — so the caller feeds [`TurnInput::PromptSubmitted`] naming it.
//! **Attribution is positional too**: the transcript ingest claims the first
//! human user line after the dispatch for that send by the same argument,
//! binding the row to that line's uuid and attributing the line (and the reply
//! that follows it) to the send's thread, rewritten text and all. Text keeps
//! one job: reporting whether the echo came back verbatim, so a new rewrite
//! shows up in the log. A send whose turn ends before any human line is
//! ingested is claimed by no line, and settles as delivered at turn end
//! ([`OrphanedSend::SettleIfUnmatched`]).
//!
//! ## Runtime-only, never persisted
//!
//! Turn state lives on each session actor's runtime state
//! (`SessionRuntime::turn`), alongside that session's pane binding, and is
//! rebuilt [`TurnState::Idle`] on boot: the pane bindings are also rebuilt
//! empty on boot, so after a server restart every session is *closed* (its
//! pane, if any, is no longer driven by this process) — and a closed session
//! cannot have a turn in flight from Delta's point of view. A session with no
//! actor therefore reads as [`TurnState::Idle`], which is exactly the state a
//! freshly-(re)opened session must start in. Persisting the old boolean was
//! in fact a liability: a stale `turn_active = 1` surviving a restart could
//! defer sends forever.
//!
//! The rebuild is **not** sound in isolation, though: the *send rows* half of
//! the single-outstanding invariant is persistent. A row that was
//! `dispatched` when the previous process died would survive into a world
//! where no turn machine awaits its echo, and — being the oldest `dispatched`
//! row — would shadow `UserPromptSubmit` correlation for every later send
//! (each prompt consumed by that ghost instead of the send it belongs to).
//! The other half
//! of the invariant is therefore restored at boot: the composition root
//! sweeps every persisted `dispatched` row back to `queued` **with the
//! restored marker set** ([`SessionStore::restore_all_dispatched`]) before
//! any session actor exists, so rebuilt-Idle turn state and the store agree
//! that nothing is outstanding. The restored row does not re-dispatch on its
//! own — the message may be stale by the time the session reopens — it stays
//! visible in the open-send list until the user explicitly releases it into
//! the normal queued flow ([`SessionStore::release_restored_send`]) or
//! cancels it.
//!
//! [`SessionStore::restore_all_dispatched`]: crate::ports::SessionStore::restore_all_dispatched
//! [`SessionStore::release_restored_send`]: crate::ports::SessionStore::release_restored_send
//!
//! ## Orphaned sends
//!
//! Some transitions abandon an outstanding (dispatched-but-unmatched) send.
//! The table reports each such send with a disposition the caller executes:
//!
//! - [`OrphanedSend::Requeue`] — **nothing was ever heard about the send**: no
//!   prompt submission arrived while it was outstanding, so its keystrokes
//!   never reached a prompt (a TUI modal swallowed the paste, a human pressed
//!   Escape) or the turn ended out from under it. Returning it to `queued`
//!   means a composed message is never silently lost: it re-dispatches intact
//!   when the session next goes idle. The worst case is benign duplication
//!   (the text was partly consumed by the pane *and* re-typed cleanly later),
//!   which the user can see and recover from — whereas cancelling would drop
//!   the message with no trace. *Bounded*, though: a send that keeps
//!   disappearing would requeue on every attempt, so the caller
//!   (`interactor::turn_input`) grants each send a finite requeue budget and
//!   parks it once the budget is spent. The count is history rather than turn
//!   state, so it lives on `SessionRuntime` (the session actor's runtime
//!   state), not in this pure table. The [`TurnInput::EchoDeadline`] watchdog
//!   is the main producer: it is what turns "no signal at all" into a signal.
//!   A *mismatched* prompt no longer lands here — position consumes the send
//!   instead of requeueing it. The other designed-for producer is the resume
//!   window: a prompt submitted while the outstanding send's keystrokes are
//!   still *held* cannot be that send's, so the send goes back to `queued`
//!   (and the caller drops the held copy, so the message is typed once).
//! - [`OrphanedSend::Cancel`] — the send can never be delivered (its pane is
//!   gone or its dispatch failed). Cancelling clears it from the open list so
//!   the failure surfaces instead of wedging the queue.
//! - [`OrphanedSend::SettleIfUnmatched`] — the send's turn ran (a prompt
//!   submission consumed it) and that turn has now ended. Normally a transcript
//!   line claimed the row mid-turn — a rewritten echo claims it just the same,
//!   attribution being positional — leaving nothing to do; when no human line
//!   was ingested at all before the turn ended (or a `/compact` swallowed it),
//!   nothing claimed the row, yet the message was still delivered, so it
//!   settles as `matched` with no uuid rather than being cancelled. Either way
//!   no stale `dispatched` row survives to shadow the next dispatch's
//!   correlation.

use crate::agent::{AgentEvent, TurnStatus};

/// The turn state of one session. See the module docs for the lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TurnState {
    /// No turn in flight; queued sends may dispatch.
    #[default]
    Idle,
    /// A send was dispatched and its `UserPromptSubmit` echo is awaited.
    AwaitingEcho { send_id: i64 },
    /// A turn is running: a matched Delta send, or external pane input (`None`).
    InFlight { send_id: Option<i64> },
}

/// Every signal that can move a session's turn state. Each existing trigger in
/// the system maps onto exactly one of these inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnInput {
    /// Delta dispatched a send: its keystrokes were typed into the pane (or are
    /// held for a resuming pane), and its `UserPromptSubmit` echo is expected.
    Dispatch { send_id: i64 },
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
    /// Such a command fires no `UserPromptSubmit` and no `Stop`, so nothing
    /// else would leave [`TurnState::AwaitingEcho`] and every later send would
    /// defer forever. This is the honest description of that end — the send
    /// was delivered and is already `matched` by the same fold — so it is
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
    EchoDeadline { send_id: i64 },
}

/// A send abandoned by a transition, with what the caller must do about it.
/// See the module docs for the disposition semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrphanedSend {
    /// Return the send to `queued`: no prompt was ever submitted for it, so it
    /// re-dispatches intact when the session next goes idle.
    Requeue(i64),
    /// Cancel the send: it can never be delivered.
    Cancel(i64),
    /// Settle the send as delivered (`matched`, no uuid) if it is still
    /// `dispatched`: its turn ran and has ended, so whether or not a transcript
    /// line ever claimed it, the message went out.
    SettleIfUnmatched(i64),
}

/// The result of applying one input: the next state, what to do with any
/// orphaned send, and whether the combination was anomalous (the caller logs it
/// loudly — an anomaly means a signal arrived that the current state says
/// should be impossible, so the table picks the safest convergent outcome).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transition {
    pub next: TurnState,
    pub orphaned: Option<OrphanedSend>,
    pub anomalous: bool,
}

impl Transition {
    fn to(next: TurnState) -> Self {
        Self {
            next,
            orphaned: None,
            anomalous: false,
        }
    }

    fn orphaning(next: TurnState, orphaned: OrphanedSend) -> Self {
        Self {
            next,
            orphaned: Some(orphaned),
            anomalous: false,
        }
    }

    fn anomaly(mut self) -> Self {
        self.anomalous = true;
        self
    }
}

/// The exhaustive transition table: every state × every input.
///
/// Total by construction (no panics, no `unreachable!`): an anomalous
/// combination converges to the safest state and is flagged so the caller logs
/// it, rather than wedging the session.
pub fn transition(state: TurnState, input: TurnInput) -> Transition {
    use OrphanedSend::{Cancel, Requeue, SettleIfUnmatched};
    use TurnInput as I;
    use TurnState as S;

    match (state, input) {
        // ---- Idle ----------------------------------------------------------
        (S::Idle, I::Dispatch { send_id }) => Transition::to(S::AwaitingEcho { send_id }),
        // A prompt with nothing outstanding is ordinary pane typing. Naming a
        // consumed send while idle is impossible (the caller only names a send
        // it read as outstanding), but if it happens the turn is genuinely
        // starting, so track it and flag the impossible part.
        (S::Idle, I::PromptSubmitted { send_id }) => {
            let t = Transition::to(S::InFlight { send_id });
            if send_id.is_some() {
                t.anomaly()
            } else {
                t
            }
        }
        // A command resolution with nothing outstanding: the fold consumed a
        // send this table does not think is in flight (a duplicate ingest of
        // the command line, most likely). The turn it would have ended is
        // already over, so converge on `Idle` exactly as a late `Stop` does.
        (S::Idle, I::CommandResolved { .. }) => Transition::to(S::Idle).anomaly(),
        // Turn-end signals while idle are no-ops: a late/duplicate Stop or
        // interrupt marker, or closing an idle session.
        (S::Idle, I::Stop) => Transition::to(S::Idle),
        (S::Idle, I::Interrupt) => Transition::to(S::Idle),
        (S::Idle, I::Close) => Transition::to(S::Idle),
        (S::Idle, I::DispatchFailed) => Transition::to(S::Idle).anomaly(),
        // A cancel while idle is meaningless: there is no outstanding send to
        // cancel. The interactor only forwards a cancel after observing an
        // `AwaitingEcho` whose send id matches, so reaching this arm means a
        // bug (or a stale message routed past the actor's guard). Converge on
        // Idle and flag it loudly.
        (S::Idle, I::Cancel { .. }) => Transition::to(S::Idle).anomaly(),
        // A deadline that lost its race: the send it named settled (matched,
        // cancelled, or orphaned by a turn end) between the sweep reading the
        // state and this transition. Nothing outstanding, nothing to do — and
        // nothing worth warning about, since the race is expected.
        (S::Idle, I::EchoDeadline { .. }) => Transition::to(S::Idle),

        // ---- AwaitingEcho --------------------------------------------------
        // A second dispatch while one is outstanding violates the
        // single-outstanding rule; keep the newer dispatch (its keystrokes are
        // the ones now in the pane) and requeue the older so it is not lost.
        (S::AwaitingEcho { send_id: old }, I::Dispatch { send_id }) => {
            Transition::orphaning(S::AwaitingEcho { send_id }, Requeue(old)).anomaly()
        }
        // A prompt arrived while a send was outstanding.
        //
        // It NAMED the outstanding send: its turn is confirmed started.
        //
        // It named a DIFFERENT send: impossible under single-outstanding;
        // credit the turn to the named send and requeue the abandoned one.
        //
        // It named NO send while one is outstanding: the resume window — the
        // send's keystrokes are still held, so a prompt arriving now cannot be
        // its. Treat the prompt as the pane input it is and requeue the
        // outstanding send so its composed message dispatches intact once this
        // turn ends. That is a designed-for outcome, not an anomaly, so it is
        // not flagged: the caller pairs the requeue with dropping the held
        // copy of the keystrokes, which is what keeps the message from being
        // typed twice.
        (S::AwaitingEcho { send_id: old }, I::PromptSubmitted { send_id }) => match send_id {
            Some(send_id) => {
                let next = S::InFlight {
                    send_id: Some(send_id),
                };
                if send_id == old {
                    Transition::to(next)
                } else {
                    Transition::orphaning(next, Requeue(old)).anomaly()
                }
            }
            None => Transition::orphaning(S::InFlight { send_id: None }, Requeue(old)),
        },
        // The outstanding send turned out to be a slash command the CLI
        // resolved on its own: the transcript ingest consumed it (it is
        // already `matched`) and reports the degenerate turn's end here. The
        // send is not orphaned — nothing is left to requeue or settle — and
        // nothing about this is anomalous. A resolution naming a different
        // send is stale (that send settled and a newer one is outstanding):
        // leave the current wait alone, flagged so the drift is visible.
        (
            S::AwaitingEcho {
                send_id: outstanding,
            },
            I::CommandResolved { send_id },
        ) => {
            if send_id == outstanding {
                Transition::to(S::Idle)
            } else {
                Transition::to(S::AwaitingEcho {
                    send_id: outstanding,
                })
                .anomaly()
            }
        }
        // The turn ended (or never existed) without the echo ever arriving:
        // the keystrokes were lost. Requeue so the message is re-typed when
        // the session is next idle.
        (S::AwaitingEcho { send_id }, I::Stop) => {
            Transition::orphaning(S::Idle, Requeue(send_id)).anomaly()
        }
        (S::AwaitingEcho { send_id }, I::Interrupt) => {
            Transition::orphaning(S::Idle, Requeue(send_id))
        }
        // The pane is gone before the echo: the send can never be delivered on
        // this pane. Cancel (not requeue): the close/failure surfaces in the
        // UI and owns recovery; silently re-typing into a future resume would
        // be surprising.
        (S::AwaitingEcho { send_id }, I::Close) => Transition::orphaning(S::Idle, Cancel(send_id)),
        (S::AwaitingEcho { send_id }, I::DispatchFailed) => {
            Transition::orphaning(S::Idle, Cancel(send_id))
        }
        // An explicit user cancel of the outstanding send. The interactor has
        // already injected `Escape` into the pane (discarding the composer
        // buffer) and validated that the request targets THIS outstanding
        // send, so the table only has to flip back to Idle and orphan the row
        // as `Cancel` — the row is marked `cancelled` in the store via the
        // existing orphan dispatch, and the next queued send dispatches on the
        // following idle-flush. A request targeting a different send id is an
        // interactor bug (the guard there only feeds `Cancel` to the turn
        // machine for the send `AwaitingEcho` is tracking; every other cancel
        // outcome — queued flip, ownerless pure transition, conflict — never
        // emits a turn input), so the mismatch arm is flagged anomalous and
        // converges on a safe no-op rather than orphaning the wrong row.
        (
            S::AwaitingEcho {
                send_id: outstanding,
            },
            I::Cancel { send_id },
        ) => {
            if send_id == outstanding {
                Transition::orphaning(S::Idle, Cancel(outstanding))
            } else {
                Transition::to(S::AwaitingEcho {
                    send_id: outstanding,
                })
                .anomaly()
            }
        }
        // The deadline fired on the send this state is waiting for: its
        // keystrokes left no trace at all. Return to `Idle` and requeue, which
        // routes the send through the *existing* budget in
        // `interactor::turn_input` — one re-type (preceded by an `Escape`, so a
        // lingering modal is dismissed and a half-landed composer draft is
        // discarded), and a park with the text handed back if that re-type is
        // swallowed too. A deadline naming a different send is stale (that send
        // settled and a newer one is outstanding): leave the current wait
        // alone.
        (
            S::AwaitingEcho {
                send_id: outstanding,
            },
            I::EchoDeadline { send_id },
        ) => {
            if send_id == outstanding {
                Transition::orphaning(S::Idle, Requeue(outstanding))
            } else {
                Transition::to(S::AwaitingEcho {
                    send_id: outstanding,
                })
            }
        }

        // ---- InFlight ------------------------------------------------------
        // Dispatching mid-turn violates the single-outstanding rule (dispatch
        // is gated on Idle); track the dispatch so its echo correlates. The
        // in-flight send (if any) already had its turn and is matched by its
        // transcript line, so it is not orphaned here.
        (S::InFlight { .. }, I::Dispatch { send_id }) => {
            Transition::to(S::AwaitingEcho { send_id }).anomaly()
        }
        // A new prompt took over the turn (Claude processed a prompt queued in
        // its own TUI). The previous turn's send was consumed and matches via
        // its transcript line; nothing to orphan. Naming a consumed send is
        // the impossible part — dispatch is gated on `Idle`, so no send can
        // have been outstanding to consume — so only that is flagged.
        (S::InFlight { .. }, I::PromptSubmitted { send_id }) => {
            let t = Transition::to(S::InFlight { send_id });
            if send_id.is_some() {
                t.anomaly()
            } else {
                t
            }
        }
        // A slash command resolved while a turn is running: a command's own
        // line consumed a send this table already moved past. The turn that is
        // running is not that send's, so the signal should be impossible here
        // — but it is still a turn end, so converge exactly as `Stop` does
        // (settling the in-flight send if there is one) rather than leaving
        // the machine in flight.
        (S::InFlight { send_id }, I::CommandResolved { .. }) => {
            let t = match send_id {
                Some(id) => Transition::orphaning(S::Idle, SettleIfUnmatched(id)),
                None => Transition::to(S::Idle),
            };
            t.anomaly()
        }
        // Turn end. A Delta send is normally claimed by a transcript line
        // during the turn, leaving this a no-op; when no human line was
        // ingested before the turn ended, nothing claimed it, yet the send was
        // still delivered — a prompt submission consumed it to get here — so it
        // settles as delivered rather than being cancelled, and no stale
        // `dispatched` row is left to break the next dispatch's
        // single-outstanding correlation.
        (S::InFlight { send_id }, I::Stop) => match send_id {
            Some(id) => Transition::orphaning(S::Idle, SettleIfUnmatched(id)),
            None => Transition::to(S::Idle),
        },
        (S::InFlight { send_id }, I::Interrupt) => match send_id {
            Some(id) => Transition::orphaning(S::Idle, SettleIfUnmatched(id)),
            None => Transition::to(S::Idle),
        },
        (S::InFlight { send_id }, I::Close) => match send_id {
            Some(id) => Transition::orphaning(S::Idle, SettleIfUnmatched(id)),
            None => Transition::to(S::Idle),
        },
        (S::InFlight { send_id }, I::DispatchFailed) => {
            let t = match send_id {
                Some(id) => Transition::orphaning(S::Idle, SettleIfUnmatched(id)),
                None => Transition::to(S::Idle),
            };
            t.anomaly()
        }
        // A cancel while a turn is running is meaningless: the dispatched
        // send (if any) already echoed and is owned by its matching transcript
        // line — the user-initiated cancel path only ever targets the
        // outstanding (pre-echo) send. The interactor guards against ever
        // forwarding the cancel in this state, so reaching this arm is a bug;
        // converge on the current state and flag it loudly.
        (S::InFlight { send_id }, I::Cancel { .. }) => {
            Transition::to(S::InFlight { send_id }).anomaly()
        }
        // The echo arrived (or an external prompt took the turn) while the
        // sweep was in flight: the wait this deadline was measuring is over, so
        // the deadline is stale. A no-op — re-typing here would double-submit a
        // prompt the model is already answering.
        (S::InFlight { send_id }, I::EchoDeadline { .. }) => {
            Transition::to(S::InFlight { send_id })
        }
    }
}

/// Map a provider-neutral [`AgentEvent`] onto the [`TurnInput`] that drives the
/// turn state machine, for the unambiguous turn-end facts only.
///
/// This is the first, isolated step of routing the runtime through the neutral
/// [`AgentEvent`] stream: it proves the turn-end mapping in a pure function,
/// without changing how the live runtime feeds the FSM (hooks and transcript
/// ingestion still drive it exactly as before). Only the three terminal
/// [`AgentEvent::TurnCompleted`] statuses have a 1:1 turn-end equivalent today:
///
/// - [`TurnStatus::Completed`] → [`TurnInput::Stop`] — the honest turn-end
///   signal the `Stop` hook produces.
/// - [`TurnStatus::Interrupted`] → [`TurnInput::Interrupt`] — the user aborted
///   the in-flight turn (no `Stop` fires).
/// - [`TurnStatus::Failed`] → [`TurnInput::Stop`] — a turn that ended on an
///   error still genuinely ended, so it takes the same honest turn-end input
///   (and orphan-send disposition) as a normal completion.
///
/// Every other [`AgentEvent`] variant returns `None`: those facts are consumed
/// by later steps and carry no turn-end meaning here.
pub fn turn_input_for_agent_event(event: &AgentEvent) -> Option<TurnInput> {
    match event {
        AgentEvent::TurnCompleted { status } => Some(match status {
            TurnStatus::Completed | TurnStatus::Failed => TurnInput::Stop,
            TurnStatus::Interrupted => TurnInput::Interrupt,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::OrphanedSend::{Cancel, Requeue, SettleIfUnmatched};
    use super::TurnInput as I;
    use super::TurnState as S;
    use super::*;

    /// One sample of every state shape and every input, for the exhaustive
    /// product test below.
    fn all_states() -> Vec<TurnState> {
        vec![
            S::Idle,
            S::AwaitingEcho { send_id: 7 },
            S::InFlight { send_id: Some(7) },
            S::InFlight { send_id: None },
        ]
    }

    /// One sample per input variant. The two id-carrying inputs whose
    /// interesting case is the MATCHING one — a prompt naming the outstanding
    /// send, a command resolving it — are sampled against the mismatching id
    /// (`9` vs the `AwaitingEcho { 7 }` sample) or the matching one (`7`)
    /// depending on which case the table below is the better home for; the
    /// other case of each pair is pinned by its own focused test underneath.
    fn all_inputs() -> Vec<TurnInput> {
        vec![
            I::Dispatch { send_id: 9 },
            // Sampled as "no send consumed": the resume-window shape, whose
            // non-anomalous requeue out of `AwaitingEcho` is the row worth
            // pinning here.
            I::PromptSubmitted { send_id: None },
            // Sampled with the outstanding id: a command resolution naming the
            // send it just consumed is the real-world shape.
            I::CommandResolved { send_id: 7 },
            I::Stop,
            I::Interrupt,
            I::Close,
            I::DispatchFailed,
            I::Cancel { send_id: 9 },
            I::EchoDeadline { send_id: 9 },
        ]
    }

    /// The full transition table, every state × every input, asserted in one
    /// place. Each row is (state, input, next, orphaned, anomalous).
    #[test]
    fn the_transition_table_is_exactly_this() {
        #[rustfmt::skip]
        let table: Vec<(TurnState, TurnInput, TurnState, Option<OrphanedSend>, bool)> = vec![
            // Idle
            (S::Idle, I::Dispatch { send_id: 9 },           S::AwaitingEcho { send_id: 9 }, None, false),
            (S::Idle, I::PromptSubmitted { send_id: None }, S::InFlight { send_id: None },  None, false),
            (S::Idle, I::CommandResolved { send_id: 7 },    S::Idle,                        None, true),
            (S::Idle, I::Stop,                              S::Idle,                        None, false),
            (S::Idle, I::Interrupt,                         S::Idle,                        None, false),
            (S::Idle, I::Close,                             S::Idle,                        None, false),
            (S::Idle, I::DispatchFailed,                    S::Idle,                        None, true),
            (S::Idle, I::Cancel { send_id: 9 },             S::Idle,                        None, true),
            (S::Idle, I::EchoDeadline { send_id: 9 },       S::Idle,                        None, false),
            // AwaitingEcho { 7 }
            (S::AwaitingEcho { send_id: 7 }, I::Dispatch { send_id: 9 },           S::AwaitingEcho { send_id: 9 }, Some(Requeue(7)), true),
            (S::AwaitingEcho { send_id: 7 }, I::PromptSubmitted { send_id: None }, S::InFlight { send_id: None },  Some(Requeue(7)), false),
            (S::AwaitingEcho { send_id: 7 }, I::CommandResolved { send_id: 7 },    S::Idle,                        None,             false),
            (S::AwaitingEcho { send_id: 7 }, I::Stop,                              S::Idle,                        Some(Requeue(7)), true),
            (S::AwaitingEcho { send_id: 7 }, I::Interrupt,                         S::Idle,                        Some(Requeue(7)), false),
            (S::AwaitingEcho { send_id: 7 }, I::Close,                             S::Idle,                        Some(Cancel(7)),  false),
            (S::AwaitingEcho { send_id: 7 }, I::DispatchFailed,                    S::Idle,                        Some(Cancel(7)),  false),
            (S::AwaitingEcho { send_id: 7 }, I::Cancel { send_id: 9 },             S::AwaitingEcho { send_id: 7 }, None,             true),
            (S::AwaitingEcho { send_id: 7 }, I::EchoDeadline { send_id: 9 },       S::AwaitingEcho { send_id: 7 }, None,             false),
            // InFlight { Some(7) }
            (S::InFlight { send_id: Some(7) }, I::Dispatch { send_id: 9 },           S::AwaitingEcho { send_id: 9 },   None,                       true),
            (S::InFlight { send_id: Some(7) }, I::PromptSubmitted { send_id: None }, S::InFlight { send_id: None },    None,                       false),
            (S::InFlight { send_id: Some(7) }, I::CommandResolved { send_id: 7 },    S::Idle,                          Some(SettleIfUnmatched(7)), true),
            (S::InFlight { send_id: Some(7) }, I::Stop,                              S::Idle,                          Some(SettleIfUnmatched(7)), false),
            (S::InFlight { send_id: Some(7) }, I::Interrupt,                         S::Idle,                          Some(SettleIfUnmatched(7)), false),
            (S::InFlight { send_id: Some(7) }, I::Close,                             S::Idle,                          Some(SettleIfUnmatched(7)), false),
            (S::InFlight { send_id: Some(7) }, I::DispatchFailed,                    S::Idle,                          Some(SettleIfUnmatched(7)), true),
            (S::InFlight { send_id: Some(7) }, I::Cancel { send_id: 9 },             S::InFlight { send_id: Some(7) }, None,                       true),
            (S::InFlight { send_id: Some(7) }, I::EchoDeadline { send_id: 9 },       S::InFlight { send_id: Some(7) }, None,                       false),
            // InFlight { None }
            (S::InFlight { send_id: None }, I::Dispatch { send_id: 9 },           S::AwaitingEcho { send_id: 9 }, None, true),
            (S::InFlight { send_id: None }, I::PromptSubmitted { send_id: None }, S::InFlight { send_id: None },  None, false),
            (S::InFlight { send_id: None }, I::CommandResolved { send_id: 7 },    S::Idle,                        None, true),
            (S::InFlight { send_id: None }, I::Stop,                              S::Idle,                        None, false),
            (S::InFlight { send_id: None }, I::Interrupt,                         S::Idle,                        None, false),
            (S::InFlight { send_id: None }, I::Close,                             S::Idle,                        None, false),
            (S::InFlight { send_id: None }, I::DispatchFailed,                    S::Idle,                        None, true),
            (S::InFlight { send_id: None }, I::Cancel { send_id: 9 },             S::InFlight { send_id: None },  None, true),
            (S::InFlight { send_id: None }, I::EchoDeadline { send_id: 9 },       S::InFlight { send_id: None },  None, false),
        ];

        // The table above must cover the whole product space exactly once.
        assert_eq!(table.len(), all_states().len() * all_inputs().len());
        for state in all_states() {
            for input in all_inputs() {
                assert!(
                    table.iter().any(|(s, i, ..)| *s == state && *i == input),
                    "table is missing ({state:?}, {input:?})"
                );
            }
        }

        for (state, input, next, orphaned, anomalous) in table {
            let got = transition(state, input);
            assert_eq!(
                got,
                Transition {
                    next,
                    orphaned,
                    anomalous
                },
                "transition({state:?}, {input:?})"
            );
        }
    }

    /// A prompt naming the outstanding send confirms that send's turn:
    /// AwaitingEcho{n} + PromptSubmitted{Some(n)} → InFlight{Some(n)}, with
    /// nothing orphaned and no anomaly.
    #[test]
    fn a_prompt_naming_the_outstanding_send_confirms_its_turn() {
        assert_eq!(
            transition(
                S::AwaitingEcho { send_id: 7 },
                I::PromptSubmitted { send_id: Some(7) }
            ),
            Transition {
                next: S::InFlight { send_id: Some(7) },
                orphaned: None,
                anomalous: false,
            }
        );
    }

    /// A prompt naming a DIFFERENT send than the outstanding one cannot happen
    /// under single-outstanding: credit the turn to the named send, requeue
    /// the abandoned one, and flag it.
    #[test]
    fn a_prompt_naming_another_send_requeues_the_outstanding_one() {
        assert_eq!(
            transition(
                S::AwaitingEcho { send_id: 7 },
                I::PromptSubmitted { send_id: Some(9) }
            ),
            Transition {
                next: S::InFlight { send_id: Some(9) },
                orphaned: Some(Requeue(7)),
                anomalous: true,
            }
        );
    }

    /// The command resolution for the outstanding send ends its degenerate
    /// turn cleanly: back to `Idle`, nothing orphaned (the fold already
    /// matched the row), and NOT anomalous — so the caller neither warns nor
    /// spends a requeue on it.
    #[test]
    fn a_command_resolution_ends_the_outstanding_sends_turn() {
        assert_eq!(
            transition(
                S::AwaitingEcho { send_id: 7 },
                I::CommandResolved { send_id: 7 }
            ),
            Transition {
                next: S::Idle,
                orphaned: None,
                anomalous: false,
            }
        );
    }

    /// A command resolution naming a different send is stale: the wait for the
    /// currently-outstanding send is left exactly as it was.
    #[test]
    fn a_stale_command_resolution_keeps_the_outstanding_send() {
        assert_eq!(
            transition(
                S::AwaitingEcho { send_id: 7 },
                I::CommandResolved { send_id: 9 }
            ),
            Transition {
                next: S::AwaitingEcho { send_id: 7 },
                orphaned: None,
                anomalous: true,
            }
        );
    }

    /// The turn-start / send-row FSM decision for a structured provider (Codex)
    /// versus Claude, in one place.
    ///
    /// Codex tracks its turn as a prompt that consumed no send
    /// (`PromptSubmitted { send_id: None }`), because `turn/start` is the
    /// authoritative confirmation and there is no echo to
    /// match. So at turn end (`Stop`) the transition orphans **nothing**, and
    /// the send it already marked matched from the provider's turn id is left
    /// alone. Claude's send rides `AwaitingEcho → InFlight { Some }`, whose
    /// `Stop` sweeps it with `SettleIfUnmatched` (a no-op once the transcript
    /// line matched it, and otherwise the settle that records the delivery).
    /// Routing a Codex send through Claude's path would therefore hand it a
    /// disposition for a row Codex has already settled its own way.
    #[test]
    fn codex_external_prompt_turn_end_orphans_nothing_unlike_claude() {
        // Codex: a send-less prompt's turn completing orphans nothing.
        let codex_in_flight = transition(S::Idle, I::PromptSubmitted { send_id: None }).next;
        assert_eq!(codex_in_flight, S::InFlight { send_id: None });
        assert_eq!(
            transition(codex_in_flight, I::Stop),
            Transition {
                next: S::Idle,
                orphaned: None,
                anomalous: false,
            },
            "a completed Codex turn cancels nothing"
        );

        // Claude: a dispatched+consumed send is swept at turn end. This is
        // exactly why Codex must NOT take this path.
        let claude_in_flight = transition(
            transition(S::Idle, I::Dispatch { send_id: 7 }).next,
            I::PromptSubmitted { send_id: Some(7) },
        )
        .next;
        assert_eq!(claude_in_flight, S::InFlight { send_id: Some(7) });
        assert_eq!(
            transition(claude_in_flight, I::Stop),
            Transition {
                next: S::Idle,
                orphaned: Some(SettleIfUnmatched(7)),
                anomalous: false,
            },
            "Claude's echo path settles its own send at turn end"
        );
    }

    /// The echo deadline for the outstanding send is the one non-stale
    /// deadline: it exits `AwaitingEcho` back to `Idle` and requeues the send
    /// (through the caller's budget), WITHOUT the anomaly flag — the watchdog
    /// firing is a designed-for outcome, not an impossible signal.
    #[test]
    fn matching_echo_deadline_requeues_the_outstanding_send() {
        assert_eq!(
            transition(
                S::AwaitingEcho { send_id: 7 },
                I::EchoDeadline { send_id: 7 }
            ),
            Transition {
                next: S::Idle,
                orphaned: Some(Requeue(7)),
                anomalous: false,
            }
        );
    }

    /// A matching explicit cancel of the outstanding send exits AwaitingEcho
    /// back to Idle and orphans the row as Cancel: the only non-anomalous
    /// cancel, since the interactor guards every other state.
    #[test]
    fn matching_cancel_exits_awaiting_echo_to_idle() {
        assert_eq!(
            transition(S::AwaitingEcho { send_id: 7 }, I::Cancel { send_id: 7 }),
            Transition {
                next: S::Idle,
                orphaned: Some(Cancel(7)),
                anomalous: false,
            }
        );
    }
}

/// FSM behaviour driven by synthetic, provider-neutral [`AgentEvent`] sequences.
///
/// These prove the neutral turn-end mapping in isolation: build an
/// [`AgentEvent`], map it with [`turn_input_for_agent_event`], apply the result
/// to the FSM, and assert the outcome equals feeding the equivalent
/// [`TurnInput`] directly. They deliberately use no provider-specific detail
/// (opaque ids are arbitrary placeholders).
#[cfg(test)]
mod agent_event_mapping_tests {
    use super::TurnState as S;
    use super::TurnStatus as St;
    use super::*;
    use crate::agent::{AgentPermissionRequest, SessionEndReason};
    use crate::interactor::PermissionDecision;
    use serde_json::json;

    /// The turn-end statuses map onto exactly the three turn-end inputs.
    #[test]
    fn turn_completed_statuses_map_to_the_turn_end_inputs() {
        assert_eq!(
            turn_input_for_agent_event(&AgentEvent::TurnCompleted {
                status: St::Completed
            }),
            Some(TurnInput::Stop),
        );
        assert_eq!(
            turn_input_for_agent_event(&AgentEvent::TurnCompleted {
                status: St::Interrupted
            }),
            Some(TurnInput::Interrupt),
        );
        // A failed turn still genuinely ended: same honest turn-end input as a
        // normal completion (mirrors the API-error abort path today).
        assert_eq!(
            turn_input_for_agent_event(&AgentEvent::TurnCompleted { status: St::Failed }),
            Some(TurnInput::Stop),
        );
    }

    /// Every non-turn-end variant maps to `None`: they carry no turn-end fact
    /// and are handled by later steps, not here.
    #[test]
    fn non_turn_end_events_map_to_none() {
        let others = [
            AgentEvent::SessionStarted {
                provider_session_id: "sid".to_owned(),
            },
            AgentEvent::SessionEnded {
                reason: SessionEndReason::Closed,
            },
            AgentEvent::SessionEnded {
                reason: SessionEndReason::ProcessExited,
            },
            AgentEvent::SessionEnded {
                reason: SessionEndReason::Failed,
            },
            AgentEvent::TurnStarted {
                provider_turn_id: None,
            },
            AgentEvent::TurnStarted {
                provider_turn_id: Some("turn".to_owned()),
            },
            AgentEvent::UserPromptAccepted {
                provider_message_id: None,
                text: "hi".to_owned(),
                at_ms: None,
            },
            AgentEvent::AssistantDelta {
                provider_item_id: "item".to_owned(),
                text: "frag".to_owned(),
            },
            AgentEvent::AssistantMessage {
                provider_item_id: "item".to_owned(),
                text: "msg".to_owned(),
                at_ms: None,
            },
            AgentEvent::ThinkingDelta {
                provider_item_id: "item".to_owned(),
                text: "frag".to_owned(),
            },
            AgentEvent::ThinkingMessage {
                provider_item_id: "item".to_owned(),
                text: "reasoning".to_owned(),
                at_ms: None,
            },
            AgentEvent::ToolStarted {
                provider_item_id: "item".to_owned(),
                name: "tool".to_owned(),
                input_json: json!({}),
                at_ms: None,
            },
            AgentEvent::ToolCompleted {
                provider_item_id: "item".to_owned(),
                output_json: json!({}),
                at_ms: None,
            },
            AgentEvent::PermissionRequested {
                request: AgentPermissionRequest {
                    request_id: "req".to_owned(),
                    tool_name: "tool".to_owned(),
                    input_json: json!({}),
                    tool_use_id: None,
                    file_change: None,
                    grant_root: None,
                },
            },
            AgentEvent::PermissionResolved {
                request_id: "req".to_owned(),
                decision: PermissionDecision::Allow,
            },
            AgentEvent::UnsupportedInteraction {
                method: "custom/method".to_owned(),
                detail_json: json!({}),
            },
            AgentEvent::Error {
                recoverable: true,
                message: "transient".to_owned(),
            },
        ];
        for event in others {
            assert_eq!(
                turn_input_for_agent_event(&event),
                None,
                "expected {event:?} to carry no turn-end input",
            );
        }
    }

    /// Applying a mapped turn-end event to the FSM produces exactly the same
    /// transition as feeding the equivalent `TurnInput` directly, from every
    /// state. This is the core equivalence: the neutral event path and the
    /// existing input path agree.
    #[test]
    fn mapped_turn_end_events_transition_identically_to_direct_inputs() {
        let states = [
            S::Idle,
            S::AwaitingEcho { send_id: 7 },
            S::InFlight { send_id: Some(7) },
            S::InFlight { send_id: None },
        ];
        let cases = [
            (St::Completed, TurnInput::Stop),
            (St::Interrupted, TurnInput::Interrupt),
            (St::Failed, TurnInput::Stop),
        ];
        for state in states {
            for (status, expected_input) in cases {
                let mapped = turn_input_for_agent_event(&AgentEvent::TurnCompleted { status })
                    .expect("turn-end event must map to an input");
                assert_eq!(mapped, expected_input, "mapping for {status:?}");
                assert_eq!(
                    transition(state, mapped),
                    transition(state, expected_input),
                    "FSM disagreed for {state:?} via {status:?}",
                );
            }
        }
    }

    /// A representative end-to-end sequence: a turn runs and then completes,
    /// driving the FSM back to `Idle` purely through mapped `AgentEvent`s.
    #[test]
    fn a_completed_turn_sequence_returns_the_fsm_to_idle() {
        // A prompt typed into the pane (consuming no send) starts a turn; the
        // neutral stream reports its completion. Only the turn-end event maps;
        // the start is represented here by the FSM's existing
        // prompt-submission input (the event that would map to it is out of
        // scope for this step).
        let mut state = transition(S::Idle, TurnInput::PromptSubmitted { send_id: None }).next;
        assert_eq!(state, S::InFlight { send_id: None });

        let stop = turn_input_for_agent_event(&AgentEvent::TurnCompleted {
            status: St::Completed,
        })
        .expect("completed turn maps to an input");
        state = transition(state, stop).next;
        assert_eq!(state, S::Idle);
    }
}
