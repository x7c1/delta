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
//! (each one mismatching against it and requeueing in a loop). The other half
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

    /// The turn-start / send-row FSM decision for a structured provider (Codex)
    /// versus Claude, in one place.
    ///
    /// Codex tracks its turn `ExternalPrompt`-style (`send_id: None`), because
    /// `turn/start` is the authoritative confirmation and there is no echo to
    /// match. So at turn end (`Stop`) the transition orphans **nothing** — a
    /// successful send is never cancelled. Claude's send rides `AwaitingEcho →
    /// InFlight { Some }`, whose `Stop` defensively `CancelIfUnmatched`es the
    /// send (a no-op once the echo matched it). Routing a Codex send through
    /// Claude's path would therefore cancel a send Codex never echoes.
    #[test]
    fn codex_external_prompt_turn_end_orphans_nothing_unlike_claude() {
        // Codex: an external-prompt turn completing orphans nothing.
        let codex_in_flight = transition(S::Idle, I::ExternalPrompt).next;
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

        // Claude (unchanged): a dispatched+echoed send is swept defensively at
        // turn end. This is exactly why Codex must NOT take this path.
        let claude_in_flight = transition(
            transition(S::Idle, I::Dispatch { send_id: 7 }).next,
            I::EchoMatched { send_id: 7 },
        )
        .next;
        assert_eq!(claude_in_flight, S::InFlight { send_id: Some(7) });
        assert_eq!(
            transition(claude_in_flight, I::Stop),
            Transition {
                next: S::Idle,
                orphaned: Some(CancelIfUnmatched(7)),
                anomalous: false,
            },
            "Claude's echo path stays byte-identical"
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
            },
            AgentEvent::AssistantDelta {
                provider_item_id: "item".to_owned(),
                text: "frag".to_owned(),
            },
            AgentEvent::AssistantMessage {
                provider_item_id: "item".to_owned(),
                text: "msg".to_owned(),
            },
            AgentEvent::ToolStarted {
                provider_item_id: "item".to_owned(),
                name: "tool".to_owned(),
                input_json: json!({}),
            },
            AgentEvent::ToolCompleted {
                provider_item_id: "item".to_owned(),
                output_json: json!({}),
            },
            AgentEvent::PermissionRequested {
                request: AgentPermissionRequest {
                    request_id: "req".to_owned(),
                    tool_name: "tool".to_owned(),
                    input_json: json!({}),
                    tool_use_id: None,
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
        // A prompt typed into the pane (external) starts a turn; the neutral
        // stream reports its completion. Only the turn-end event maps; the
        // start is represented here by the FSM's existing external-prompt input
        // (the event that would map to it is out of scope for this step).
        let mut state = transition(S::Idle, TurnInput::ExternalPrompt).next;
        assert_eq!(state, S::InFlight { send_id: None });

        let stop = turn_input_for_agent_event(&AgentEvent::TurnCompleted {
            status: St::Completed,
        })
        .expect("completed turn maps to an input");
        state = transition(state, stop).next;
        assert_eq!(state, S::Idle);
    }
}
