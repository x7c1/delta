//! The turn machine's transition table and the [`Transition`] it yields.

use super::{OrphanedSend, TurnInput, TurnState};

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
        (
            S::Idle,
            I::Dispatch {
                send_id,
                slash_command,
            },
        ) => Transition::to(S::AwaitingEcho {
            send_id,
            slash_command,
        }),
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
        (
            S::AwaitingEcho { send_id: old, .. },
            I::Dispatch {
                send_id,
                slash_command,
            },
        ) => Transition::orphaning(
            S::AwaitingEcho {
                send_id,
                slash_command,
            },
            Requeue(old),
        )
        .anomaly(),
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
        (S::AwaitingEcho { send_id: old, .. }, I::PromptSubmitted { send_id }) => match send_id {
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
            awaiting @ S::AwaitingEcho {
                send_id: outstanding,
                ..
            },
            I::CommandResolved { send_id },
        ) => {
            if send_id == outstanding {
                Transition::to(S::Idle)
            } else {
                Transition::to(awaiting).anomaly()
            }
        }
        // The turn ended (or never existed) without the echo ever arriving:
        // the keystrokes were lost. Requeue so the message is re-typed when
        // the session is next idle.
        (S::AwaitingEcho { send_id, .. }, I::Stop) => {
            Transition::orphaning(S::Idle, Requeue(send_id)).anomaly()
        }
        (S::AwaitingEcho { send_id, .. }, I::Interrupt) => {
            Transition::orphaning(S::Idle, Requeue(send_id))
        }
        // The pane is gone before the echo: the send can never be delivered on
        // this pane. Cancel (not requeue): the close/failure surfaces in the
        // UI and owns recovery; silently re-typing into a future resume would
        // be surprising.
        (S::AwaitingEcho { send_id, .. }, I::Close) => {
            Transition::orphaning(S::Idle, Cancel(send_id))
        }
        (S::AwaitingEcho { send_id, .. }, I::DispatchFailed) => {
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
            awaiting @ S::AwaitingEcho {
                send_id: outstanding,
                ..
            },
            I::Cancel { send_id },
        ) => {
            if send_id == outstanding {
                Transition::orphaning(S::Idle, Cancel(outstanding))
            } else {
                Transition::to(awaiting).anomaly()
            }
        }
        // The deadline fired on the send this state is waiting for: its
        // keystrokes left no trace at all. Return to `Idle` and requeue, which
        // routes the send through the *existing* budget in
        // `interactor::turn_input` — one re-type (preceded by an `Escape`, so a
        // lingering modal is dismissed and a half-landed composer draft is
        // discarded), and a park — the row held in the queue for an explicit
        // release — if that re-type is swallowed too.
        //
        // A slash-command send is the exception: silence is how a local
        // command (`/cost`, `/model`, …) ends from Claude Code 2.1.286 on — no
        // hook, no transcript line — so the command ran, and re-typing it
        // would run it twice. Settle it as delivered and return to `Idle`
        // without spending any requeue budget.
        //
        // A deadline naming a different send is stale (that send settled and
        // a newer one is outstanding): leave the current wait alone.
        (
            awaiting @ S::AwaitingEcho {
                send_id: outstanding,
                slash_command,
            },
            I::EchoDeadline { send_id },
        ) => {
            if send_id != outstanding {
                Transition::to(awaiting)
            } else if slash_command {
                Transition::orphaning(S::Idle, SettleIfUnmatched(outstanding))
            } else {
                Transition::orphaning(S::Idle, Requeue(outstanding))
            }
        }

        // ---- InFlight ------------------------------------------------------
        // Dispatching mid-turn violates the single-outstanding rule (dispatch
        // is gated on Idle); track the dispatch so its echo correlates. The
        // in-flight send (if any) already had its turn and is matched by its
        // transcript line, so it is not orphaned here.
        (
            S::InFlight { .. },
            I::Dispatch {
                send_id,
                slash_command,
            },
        ) => Transition::to(S::AwaitingEcho {
            send_id,
            slash_command,
        })
        .anomaly(),
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

#[cfg(test)]
mod tests {
    use super::OrphanedSend::{Cancel, Requeue, SettleIfUnmatched};
    use super::TurnInput as I;
    use super::TurnState as S;
    use super::*;

    /// The wait for a plain-prompt send `send_id`.
    fn awaiting(send_id: i64) -> TurnState {
        S::AwaitingEcho {
            send_id,
            slash_command: false,
        }
    }

    /// The wait for a slash-command send `send_id`.
    fn awaiting_command(send_id: i64) -> TurnState {
        S::AwaitingEcho {
            send_id,
            slash_command: true,
        }
    }

    /// The dispatch of a plain-prompt send `send_id`.
    fn dispatch(send_id: i64) -> TurnInput {
        I::Dispatch {
            send_id,
            slash_command: false,
        }
    }

    /// One sample of every state shape and every input, for the exhaustive
    /// product test below.
    fn all_states() -> Vec<TurnState> {
        vec![
            S::Idle,
            awaiting(7),
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
            dispatch(9),
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
            (S::Idle, dispatch(9),                          awaiting(9),                   None, false),
            (S::Idle, I::PromptSubmitted { send_id: None }, S::InFlight { send_id: None }, None, false),
            (S::Idle, I::CommandResolved { send_id: 7 },    S::Idle,                       None, true),
            (S::Idle, I::Stop,                              S::Idle,                       None, false),
            (S::Idle, I::Interrupt,                         S::Idle,                       None, false),
            (S::Idle, I::Close,                             S::Idle,                       None, false),
            (S::Idle, I::DispatchFailed,                    S::Idle,                       None, true),
            (S::Idle, I::Cancel { send_id: 9 },             S::Idle,                       None, true),
            (S::Idle, I::EchoDeadline { send_id: 9 },       S::Idle,                       None, false),
            // AwaitingEcho { 7 }
            (awaiting(7), dispatch(9),                          awaiting(9),                   Some(Requeue(7)), true),
            (awaiting(7), I::PromptSubmitted { send_id: None }, S::InFlight { send_id: None }, Some(Requeue(7)), false),
            (awaiting(7), I::CommandResolved { send_id: 7 },    S::Idle,                       None,             false),
            (awaiting(7), I::Stop,                              S::Idle,                       Some(Requeue(7)), true),
            (awaiting(7), I::Interrupt,                         S::Idle,                       Some(Requeue(7)), false),
            (awaiting(7), I::Close,                             S::Idle,                       Some(Cancel(7)),  false),
            (awaiting(7), I::DispatchFailed,                    S::Idle,                       Some(Cancel(7)),  false),
            (awaiting(7), I::Cancel { send_id: 9 },             awaiting(7),                   None,             true),
            (awaiting(7), I::EchoDeadline { send_id: 9 },       awaiting(7),                   None,             false),
            // InFlight { Some(7) }
            (S::InFlight { send_id: Some(7) }, dispatch(9),                          awaiting(9),                      None,                       true),
            (S::InFlight { send_id: Some(7) }, I::PromptSubmitted { send_id: None }, S::InFlight { send_id: None },    None,                       false),
            (S::InFlight { send_id: Some(7) }, I::CommandResolved { send_id: 7 },    S::Idle,                          Some(SettleIfUnmatched(7)), true),
            (S::InFlight { send_id: Some(7) }, I::Stop,                              S::Idle,                          Some(SettleIfUnmatched(7)), false),
            (S::InFlight { send_id: Some(7) }, I::Interrupt,                         S::Idle,                          Some(SettleIfUnmatched(7)), false),
            (S::InFlight { send_id: Some(7) }, I::Close,                             S::Idle,                          Some(SettleIfUnmatched(7)), false),
            (S::InFlight { send_id: Some(7) }, I::DispatchFailed,                    S::Idle,                          Some(SettleIfUnmatched(7)), true),
            (S::InFlight { send_id: Some(7) }, I::Cancel { send_id: 9 },             S::InFlight { send_id: Some(7) }, None,                       true),
            (S::InFlight { send_id: Some(7) }, I::EchoDeadline { send_id: 9 },       S::InFlight { send_id: Some(7) }, None,                       false),
            // InFlight { None }
            (S::InFlight { send_id: None }, dispatch(9),                          awaiting(9),                   None, true),
            (S::InFlight { send_id: None }, I::PromptSubmitted { send_id: None }, S::InFlight { send_id: None }, None, false),
            (S::InFlight { send_id: None }, I::CommandResolved { send_id: 7 },    S::Idle,                       None, true),
            (S::InFlight { send_id: None }, I::Stop,                              S::Idle,                       None, false),
            (S::InFlight { send_id: None }, I::Interrupt,                         S::Idle,                       None, false),
            (S::InFlight { send_id: None }, I::Close,                             S::Idle,                       None, false),
            (S::InFlight { send_id: None }, I::DispatchFailed,                    S::Idle,                       None, true),
            (S::InFlight { send_id: None }, I::Cancel { send_id: 9 },             S::InFlight { send_id: None }, None, true),
            (S::InFlight { send_id: None }, I::EchoDeadline { send_id: 9 },       S::InFlight { send_id: None }, None, false),
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
            transition(awaiting(7), I::PromptSubmitted { send_id: Some(7) }),
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
            transition(awaiting(7), I::PromptSubmitted { send_id: Some(9) }),
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
            transition(awaiting(7), I::CommandResolved { send_id: 7 }),
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
            transition(awaiting(7), I::CommandResolved { send_id: 9 }),
            Transition {
                next: awaiting(7),
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
            transition(S::Idle, dispatch(7)).next,
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

    /// The echo deadline for an outstanding plain-prompt send is the one
    /// non-stale deadline: it exits `AwaitingEcho` back to `Idle` and requeues
    /// the send (through the caller's budget), WITHOUT the anomaly flag — the
    /// watchdog firing is a designed-for outcome, not an impossible signal.
    #[test]
    fn matching_echo_deadline_requeues_the_outstanding_send() {
        assert_eq!(
            transition(awaiting(7), I::EchoDeadline { send_id: 7 }),
            Transition {
                next: S::Idle,
                orphaned: Some(Requeue(7)),
                anomalous: false,
            }
        );
    }

    /// A slash-command send that nothing was heard about is a local command
    /// that ran (Claude Code 2.1.286+ records nothing for one): its deadline
    /// settles the send as delivered and returns to `Idle` — no `Requeue`, so
    /// the command is never typed a second time — and is not anomalous.
    #[test]
    fn matching_echo_deadline_settles_an_outstanding_slash_command() {
        assert_eq!(
            transition(awaiting_command(7), I::EchoDeadline { send_id: 7 }),
            Transition {
                next: S::Idle,
                orphaned: Some(SettleIfUnmatched(7)),
                anomalous: false,
            }
        );
    }

    /// A stale deadline leaves a slash-command wait exactly as it was, like any
    /// other wait.
    #[test]
    fn a_stale_echo_deadline_keeps_the_slash_command_wait() {
        assert_eq!(
            transition(awaiting_command(7), I::EchoDeadline { send_id: 9 }),
            Transition {
                next: awaiting_command(7),
                orphaned: None,
                anomalous: false,
            }
        );
    }

    /// The kind of the dispatched send is carried into the wait, derived from
    /// its text by the fold's own slash-command predicate.
    #[test]
    fn dispatch_carries_the_sends_kind_into_the_wait() {
        assert_eq!(
            transition(S::Idle, TurnInput::dispatch(7, "/cost")).next,
            awaiting_command(7)
        );
        assert_eq!(
            transition(S::Idle, TurnInput::dispatch(7, "explain /cost")).next,
            awaiting(7)
        );
    }

    /// A matching explicit cancel of the outstanding send exits AwaitingEcho
    /// back to Idle and orphans the row as Cancel: the only non-anomalous
    /// cancel, since the interactor guards every other state.
    #[test]
    fn matching_cancel_exits_awaiting_echo_to_idle() {
        assert_eq!(
            transition(awaiting(7), I::Cancel { send_id: 7 }),
            Transition {
                next: S::Idle,
                orphaned: Some(Cancel(7)),
                anomalous: false,
            }
        );
    }
}
