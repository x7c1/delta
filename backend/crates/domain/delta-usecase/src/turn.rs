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
//!   `dispatched` send exists per session, so `UserPromptSubmit` correlation is
//!   a comparison against that one outstanding send, not a FIFO scan).
//! - [`TurnState::AwaitingEcho`] — Delta dispatched a send (its keystrokes were
//!   typed, or are held for a resuming pane) and is waiting for the
//!   `UserPromptSubmit` hook to echo it back.
//! - [`TurnState::InFlight`] — a turn is running: either the echoed Delta send
//!   (`send_id: Some`) or a prompt typed straight into the pane
//!   (`send_id: None`).
//!
//! ## Runtime-only, never persisted
//!
//! Turn state lives on each session actor's runtime state
//! (`SessionRuntime::turn`), alongside that session's pane binding, and is
//! rebuilt [`TurnState::Idle`] on boot. That is correct by construction: the
//! pane bindings are also rebuilt empty on boot, so after a server restart
//! every session is *closed* (its pane, if any, is no longer driven by this
//! process) — and a closed session cannot have a turn in flight from Delta's
//! point of view. A session with no actor therefore reads as
//! [`TurnState::Idle`], which is exactly the state a freshly-(re)opened
//! session must start in. Persisting the old boolean was in fact a liability: a
//! stale `turn_active = 1` surviving a restart could defer sends forever.
//!
//! ## Orphaned sends
//!
//! Some transitions abandon an outstanding (dispatched-but-unmatched) send.
//! The table reports each such send with a disposition the caller executes:
//!
//! - [`OrphanedSend::Requeue`] — the send's turn never started (its echo never
//!   arrived: the prompt was mangled by interleaved pane typing, or the turn
//!   ended out from under it). Returning it to `queued` means a composed
//!   message is never silently lost: it re-dispatches intact when the session
//!   next goes idle. The worst case is benign duplication (the text was partly
//!   consumed inside a mangled external prompt *and* re-typed cleanly later),
//!   which the user can see and recover from — whereas cancelling would drop
//!   the message with no trace. This is the deliberate mismatch semantics.
//! - [`OrphanedSend::Cancel`] — the send can never be delivered (its pane is
//!   gone or its dispatch failed). Cancelling clears it from the open list so
//!   the failure surfaces instead of wedging the queue.
//! - [`OrphanedSend::CancelIfUnmatched`] — the send's turn ran (it was echoed)
//!   and normally matched its transcript line before the turn ended; this is a
//!   defensive sweep for the rare case it never did, so a stale `dispatched`
//!   row cannot break the single-outstanding invariant for the next dispatch.

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
    /// `UserPromptSubmit` arrived and its prompt text equals the outstanding
    /// send's text — the dispatched send's turn is confirmed started.
    EchoMatched { send_id: i64 },
    /// `UserPromptSubmit` arrived with no matching outstanding send: a prompt
    /// typed straight into the pane (or a mismatched echo — see the orphan
    /// semantics on [`OrphanedSend::Requeue`]).
    ExternalPrompt,
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
}

/// A send abandoned by a transition, with what the caller must do about it.
/// See the module docs for the disposition semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrphanedSend {
    /// Return the send to `queued`: it never echoed, so it re-dispatches intact
    /// when the session next goes idle.
    Requeue(i64),
    /// Cancel the send: it can never be delivered.
    Cancel(i64),
    /// Cancel the send only if it is still `dispatched` (defensive sweep; it
    /// normally matched its transcript line during the turn).
    CancelIfUnmatched(i64),
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
    use OrphanedSend::{Cancel, CancelIfUnmatched, Requeue};
    use TurnInput as I;
    use TurnState as S;

    match (state, input) {
        // ---- Idle ----------------------------------------------------------
        (S::Idle, I::Dispatch { send_id }) => Transition::to(S::AwaitingEcho { send_id }),
        // An echo with nothing outstanding: impossible (the caller only emits
        // EchoMatched after comparing against an outstanding send), but if it
        // happens the turn is genuinely starting, so track it.
        (S::Idle, I::EchoMatched { send_id }) => Transition::to(S::InFlight {
            send_id: Some(send_id),
        })
        .anomaly(),
        (S::Idle, I::ExternalPrompt) => Transition::to(S::InFlight { send_id: None }),
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

        // ---- AwaitingEcho --------------------------------------------------
        // A second dispatch while one is outstanding violates the
        // single-outstanding rule; keep the newer dispatch (its keystrokes are
        // the ones now in the pane) and requeue the older so it is not lost.
        (S::AwaitingEcho { send_id: old }, I::Dispatch { send_id }) => {
            Transition::orphaning(S::AwaitingEcho { send_id }, Requeue(old)).anomaly()
        }
        (S::AwaitingEcho { send_id: old }, I::EchoMatched { send_id }) => {
            let next = S::InFlight {
                send_id: Some(send_id),
            };
            if send_id == old {
                Transition::to(next)
            } else {
                // The caller matched a different send than the one this table
                // thinks is outstanding — should be impossible under
                // single-outstanding; requeue the abandoned one.
                Transition::orphaning(next, Requeue(old)).anomaly()
            }
        }
        // The echo MISMATCHED: the prompt that submitted is not the send Delta
        // typed (interleaved pane typing mangled it, most likely). Treat the
        // prompt as the external input it textually is, and requeue the
        // outstanding send so the composed message re-dispatches intact once
        // this external turn ends. (Loud-log via `anomalous`.)
        (S::AwaitingEcho { send_id }, I::ExternalPrompt) => {
            Transition::orphaning(S::InFlight { send_id: None }, Requeue(send_id)).anomaly()
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
        (S::AwaitingEcho { send_id }, I::Close) => {
            Transition::orphaning(S::Idle, Cancel(send_id))
        }
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
        // interactor bug (the guard there would have rejected it as
        // `SendNotCancellable`), so the mismatch arm is flagged anomalous and
        // converges on a safe no-op rather than orphaning the wrong row.
        (S::AwaitingEcho { send_id: outstanding }, I::Cancel { send_id }) => {
            if send_id == outstanding {
                Transition::orphaning(S::Idle, Cancel(outstanding))
            } else {
                Transition::to(S::AwaitingEcho {
                    send_id: outstanding,
                })
                .anomaly()
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
        (S::InFlight { .. }, I::EchoMatched { send_id }) => Transition::to(S::InFlight {
            send_id: Some(send_id),
        })
        .anomaly(),
        // A new prompt took over the turn (Claude processed a prompt queued in
        // its own TUI). The previous turn's send was echoed and matches via
        // its transcript line; nothing to orphan.
        (S::InFlight { .. }, I::ExternalPrompt) => Transition::to(S::InFlight { send_id: None }),
        // Turn end. A Delta send normally matched its transcript line during
        // the turn; sweep it defensively in case that line never appeared, so
        // a stale `dispatched` row cannot break the next dispatch's
        // single-outstanding correlation.
        (S::InFlight { send_id }, I::Stop) => match send_id {
            Some(id) => Transition::orphaning(S::Idle, CancelIfUnmatched(id)),
            None => Transition::to(S::Idle),
        },
        (S::InFlight { send_id }, I::Interrupt) => match send_id {
            Some(id) => Transition::orphaning(S::Idle, CancelIfUnmatched(id)),
            None => Transition::to(S::Idle),
        },
        (S::InFlight { send_id }, I::Close) => match send_id {
            Some(id) => Transition::orphaning(S::Idle, CancelIfUnmatched(id)),
            None => Transition::to(S::Idle),
        },
        (S::InFlight { send_id }, I::DispatchFailed) => {
            let t = match send_id {
                Some(id) => Transition::orphaning(S::Idle, CancelIfUnmatched(id)),
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
    }
}

#[cfg(test)]
mod tests {
    use super::OrphanedSend::{Cancel, CancelIfUnmatched, Requeue};
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

    fn all_inputs() -> Vec<TurnInput> {
        vec![
            I::Dispatch { send_id: 9 },
            I::EchoMatched { send_id: 9 },
            I::ExternalPrompt,
            I::Stop,
            I::Interrupt,
            I::Close,
            I::DispatchFailed,
            I::Cancel { send_id: 9 },
        ]
    }

    /// The full transition table, every state × every input, asserted in one
    /// place. Each row is (state, input, next, orphaned, anomalous).
    #[test]
    fn the_transition_table_is_exactly_this() {
        #[rustfmt::skip]
        let table: Vec<(TurnState, TurnInput, TurnState, Option<OrphanedSend>, bool)> = vec![
            // Idle
            (S::Idle, I::Dispatch { send_id: 9 },    S::AwaitingEcho { send_id: 9 },    None,                          false),
            (S::Idle, I::EchoMatched { send_id: 9 }, S::InFlight { send_id: Some(9) },  None,                          true),
            (S::Idle, I::ExternalPrompt,             S::InFlight { send_id: None },     None,                          false),
            (S::Idle, I::Stop,                       S::Idle,                           None,                          false),
            (S::Idle, I::Interrupt,                  S::Idle,                           None,                          false),
            (S::Idle, I::Close,                      S::Idle,                           None,                          false),
            (S::Idle, I::DispatchFailed,             S::Idle,                           None,                          true),
            (S::Idle, I::Cancel { send_id: 9 },      S::Idle,                           None,                          true),
            // AwaitingEcho { 7 }
            (S::AwaitingEcho { send_id: 7 }, I::Dispatch { send_id: 9 },    S::AwaitingEcho { send_id: 9 },   Some(Requeue(7)), true),
            (S::AwaitingEcho { send_id: 7 }, I::EchoMatched { send_id: 9 }, S::InFlight { send_id: Some(9) }, Some(Requeue(7)), true),
            (S::AwaitingEcho { send_id: 7 }, I::ExternalPrompt,             S::InFlight { send_id: None },    Some(Requeue(7)), true),
            (S::AwaitingEcho { send_id: 7 }, I::Stop,                       S::Idle,                          Some(Requeue(7)), true),
            (S::AwaitingEcho { send_id: 7 }, I::Interrupt,                  S::Idle,                          Some(Requeue(7)), false),
            (S::AwaitingEcho { send_id: 7 }, I::Close,                      S::Idle,                          Some(Cancel(7)),  false),
            (S::AwaitingEcho { send_id: 7 }, I::DispatchFailed,             S::Idle,                          Some(Cancel(7)),  false),
            (S::AwaitingEcho { send_id: 7 }, I::Cancel { send_id: 9 },      S::AwaitingEcho { send_id: 7 },   None,             true),
            // InFlight { Some(7) }
            (S::InFlight { send_id: Some(7) }, I::Dispatch { send_id: 9 },    S::AwaitingEcho { send_id: 9 },   None,                        true),
            (S::InFlight { send_id: Some(7) }, I::EchoMatched { send_id: 9 }, S::InFlight { send_id: Some(9) }, None,                        true),
            (S::InFlight { send_id: Some(7) }, I::ExternalPrompt,             S::InFlight { send_id: None },    None,                        false),
            (S::InFlight { send_id: Some(7) }, I::Stop,                       S::Idle,                          Some(CancelIfUnmatched(7)),  false),
            (S::InFlight { send_id: Some(7) }, I::Interrupt,                  S::Idle,                          Some(CancelIfUnmatched(7)),  false),
            (S::InFlight { send_id: Some(7) }, I::Close,                      S::Idle,                          Some(CancelIfUnmatched(7)),  false),
            (S::InFlight { send_id: Some(7) }, I::DispatchFailed,             S::Idle,                          Some(CancelIfUnmatched(7)),  true),
            (S::InFlight { send_id: Some(7) }, I::Cancel { send_id: 9 },      S::InFlight { send_id: Some(7) }, None,                        true),
            // InFlight { None }
            (S::InFlight { send_id: None }, I::Dispatch { send_id: 9 },    S::AwaitingEcho { send_id: 9 },   None, true),
            (S::InFlight { send_id: None }, I::EchoMatched { send_id: 9 }, S::InFlight { send_id: Some(9) }, None, true),
            (S::InFlight { send_id: None }, I::ExternalPrompt,             S::InFlight { send_id: None },    None, false),
            (S::InFlight { send_id: None }, I::Stop,                       S::Idle,                          None, false),
            (S::InFlight { send_id: None }, I::Interrupt,                  S::Idle,                          None, false),
            (S::InFlight { send_id: None }, I::Close,                      S::Idle,                          None, false),
            (S::InFlight { send_id: None }, I::DispatchFailed,             S::Idle,                          None, true),
            (S::InFlight { send_id: None }, I::Cancel { send_id: 9 },      S::InFlight { send_id: None },    None, true),
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

    /// A matching echo for the outstanding send is the one non-anomalous echo:
    /// AwaitingEcho{n} + EchoMatched{n} → InFlight{Some(n)}.
    #[test]
    fn matching_echo_confirms_the_dispatched_turn() {
        assert_eq!(
            transition(
                S::AwaitingEcho { send_id: 7 },
                I::EchoMatched { send_id: 7 }
            ),
            Transition {
                next: S::InFlight { send_id: Some(7) },
                orphaned: None,
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
            transition(
                S::AwaitingEcho { send_id: 7 },
                I::Cancel { send_id: 7 }
            ),
            Transition {
                next: S::Idle,
                orphaned: Some(Cancel(7)),
                anomalous: false,
            }
        );
    }
}
