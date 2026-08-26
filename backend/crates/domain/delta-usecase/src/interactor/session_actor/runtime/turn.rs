//! The turn state machine's runtime application: applying one input,
//! reading the current phase, the echo-deadline stamp, and the turn-end /
//! session-deletion clears.

use std::time::{Duration, Instant};

use crate::turn::{transition, Transition, TurnInput, TurnState};

use super::SessionRuntime;

/// How many times one send may be returned to `queued` by the turn machine
/// before Delta stops re-dispatching it and parks it instead.
///
/// The budget covers exactly one situation: *nobody ever heard about the
/// send*. No prompt submission arrived while it was outstanding, so its
/// keystrokes never became a prompt at all. (A prompt that does arrive consumes
/// the send by position, whatever its text says, so a rewritten echo no longer
/// spends anything here — that used to be this budget's main customer.)
///
/// One retry, not zero: keystrokes swallowed once — a TUI modal eating the
/// paste and its trailing Enter, a compaction landing on top of them — really
/// do arrive on the next attempt, and losing a composed message to a single
/// hiccup would be worse than the retry. Not more than one: whatever ate them
/// may still be there, each attempt costs a full model turn, and a send that
/// vanished twice is better handed back to the user (parked, with its text)
/// than re-typed forever. Two dispatches are enough to tell the two apart.
pub const MAX_REQUEUES_PER_SEND: u32 = 1;

/// How long a dispatched send may wait for its `UserPromptSubmit` echo before
/// the watchdog gives up on hearing anything and feeds the turn machine a
/// [`TurnInput::EchoDeadline`].
///
/// Deliberately generous, like the launch deadlines: the real echo loop
/// (keystrokes → tmux → the TUI submitting → the hook reaching Delta) has been
/// measured at worst a handful of seconds under load, and an auto-compaction
/// starting the instant the keystrokes land stretches that window further —
/// but that case has its own, cheaper recovery (the compact re-dispatch
/// re-types with no budget spent and re-stamps this deadline), so this value
/// only has to outlast the *tail* of it. Anything past a minute of complete
/// silence is not a slow echo, it is a lost one: the keystrokes were swallowed
/// by something Delta cannot see (a TUI modal eating the paste and its
/// trailing Enter, a human pressing Escape in the attached pane), and no event
/// will ever arrive to say so.
///
/// Overridable via `DELTA_ECHO_DEADLINE_MS` (see the server's
/// `launch_from_env`) so the fake end-to-end suite can exercise the whole
/// retry-then-park path in seconds.
pub const ECHO_DEADLINE: Duration = Duration::from_secs(60);

impl SessionRuntime {
    /// The session's current turn state.
    pub fn turn(&self) -> TurnState {
        self.turn
    }

    /// Apply one input to the turn state machine, returning the full
    /// transition (the caller executes the orphan disposition and logs
    /// anomalies). The transition table lives in the `turn` module.
    ///
    /// A transition back to [`TurnState::Idle`] (stop, interrupt, close) also
    /// drops every pending permission dialog and the pending question: they all
    /// blocked that turn, so the turn ending — however it ended — makes them moot.
    /// This is the same lifecycle the browser notices have. The provider has
    /// already settled or abandoned those requests by then; Delta only drops its
    /// mirror of them.
    pub fn apply_turn(&mut self, input: TurnInput) -> Transition {
        let previous = self.turn;
        let result = transition(self.turn, input);
        self.turn = result.next;
        // Keep the echo-deadline stamp in lockstep with the state it measures:
        // stamped on ENTERING a wait (a fresh dispatch, or a dispatch that
        // replaced an older one), cleared on leaving it. Re-entering the same
        // wait — the no-op arms that keep `AwaitingEcho { same id }` — keeps
        // the original stamp, so a stray stale input cannot postpone the
        // deadline indefinitely.
        match result.next {
            TurnState::AwaitingEcho { .. } if previous != result.next => {
                self.awaiting_echo_since = Some(Instant::now());
            }
            TurnState::AwaitingEcho { .. } => {}
            _ => self.awaiting_echo_since = None,
        }
        // A matched send has left the outstanding set for good; dropping its
        // budget here (not only on the orphan dispositions) keeps the map
        // bounded by the sends in flight, not by the session's lifetime.
        if let TurnInput::EchoMatched { send_id } = input {
            self.forget_requeues(send_id);
        }
        if result.next == TurnState::Idle {
            self.pending_permissions.clear();
            self.pending_question = None;
            // A FOREGROUND subagent cannot outlive the turn that spawned it:
            // once the turn ends (stop, interrupt, close) any still-running
            // foreground entry is moot, so drop it. This also covers the case
            // where a foreground `PostToolUse(Agent)` was somehow missed — the
            // turn end clears it rather than leaving a stuck indicator. A
            // BACKGROUND subagent (`run_in_background: true`) deliberately
            // outlives the launching turn: it keeps running after the turn
            // returns to idle, so it is kept here and removed only when its
            // completion `<task-notification>` is folded.
            self.running_subagents.retain(|s| s.background);
            // The provisional live preview belongs to the turn that just ended;
            // the persisted assistant message (ingested by the transcript sync)
            // now renders instead, so drop the preview to avoid a duplicate.
            self.streaming_message = None;
        }
        result
    }

    /// Drop the turn state without any orphan handling. Used when the session
    /// row itself is being deleted (its sends go with it by cascade).
    ///
    /// Unlike [`Self::apply_turn`], this clears the WHOLE running set including
    /// background subagents: the session is being deleted, so no later
    /// completion notification can arrive to finish a background entry — keeping
    /// one would pin a doomed actor alive forever.
    pub fn forget_turn(&mut self) {
        self.turn = TurnState::Idle;
        self.awaiting_echo_since = None;
        self.pending_permissions.clear();
        self.pending_question = None;
        self.running_subagents.clear();
        self.streaming_message = None;
        self.requeues_per_send.clear();
    }

    /// Spend one requeue from `send_id`'s budget, reporting whether the send
    /// may still be returned to `queued` (and so re-dispatched on the next
    /// idle). `false` means the budget is exhausted: the caller must park the
    /// send instead of requeueing it, or the dispatch⇄silence cycle never ends
    /// (whatever swallowed the keystrokes swallows the re-typed ones too). See
    /// [`MAX_REQUEUES_PER_SEND`] and the `requeues_per_send` field docs for why
    /// the cap is where it is.
    pub fn claim_requeue(&mut self, send_id: i64) -> bool {
        let spent = self.requeues_per_send.entry(send_id).or_insert(0);
        *spent += 1;
        *spent <= MAX_REQUEUES_PER_SEND
    }

    /// Drop `send_id`'s requeue budget: the send left the outstanding set
    /// (matched, cancelled, or parked), so its retry history is moot.
    pub fn forget_requeues(&mut self, send_id: i64) {
        self.requeues_per_send.remove(&send_id);
    }

    /// Restart the echo-deadline clock for the send currently being awaited.
    ///
    /// [`Self::apply_turn`] stamps the clock when the wait *begins*, which
    /// covers every path that types keystrokes as part of a transition. Two
    /// paths re-type an already-outstanding send without any transition at all
    /// — the compact re-dispatch, and the resume settle typing a held first
    /// prompt — and each of those restarts the wait for real, so each calls
    /// this. Without it, a send held across a slow resume would be measured
    /// from the moment its row was written rather than from the moment its
    /// keystrokes actually reached the pane.
    ///
    /// A no-op when no send is being awaited: there is no wait to restart.
    pub fn restamp_awaiting_echo(&mut self) {
        if matches!(self.turn, TurnState::AwaitingEcho { .. }) {
            self.awaiting_echo_since = Some(Instant::now());
        }
    }

    /// The outstanding send whose echo deadline has passed as of `now`, if any
    /// — the watchdog's read half (the sweep feeds the id back as
    /// [`TurnInput::EchoDeadline`]).
    ///
    /// `now` is supplied by the caller rather than read here so the sweep is
    /// deterministic under test, exactly like the launch watchdog's
    /// `take_stale_pending`. Returns `None` unless a send is genuinely being
    /// awaited AND its wait has run past `deadline`.
    ///
    /// A session inside its resume-readiness window is never reported: its
    /// first prompt's keystrokes are deliberately *held* (typing into a pane
    /// that is not yet accepting input would lose them), so there is nothing
    /// in flight to have gone missing. The wait that this deadline measures
    /// starts when the resume settles and the held prompt is actually typed —
    /// which is one of the two [`Self::restamp_awaiting_echo`] call sites.
    pub fn expired_echo_deadline(&self, now: Instant, deadline: Duration) -> Option<i64> {
        if self.is_resuming() {
            return None;
        }
        let TurnState::AwaitingEcho { send_id } = self.turn else {
            return None;
        };
        let since = self.awaiting_echo_since?;
        (now.duration_since(since) >= deadline).then_some(send_id)
    }
}
