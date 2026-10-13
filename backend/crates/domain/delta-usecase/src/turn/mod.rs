//! The per-session turn state machine.
//!
//! A *turn* is one prompt→response round trip in a Claude Code session. Delta
//! used to track it as a persisted boolean (`session.turn_active`) mutated from
//! several scattered call sites; this module replaces that with one explicit
//! state machine whose every transition is defined — and unit-tested — in a
//! single place ([`transition()`]).
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
//!   `UserPromptSubmit` hook to echo it back. The state remembers whether the
//!   send is a slash command, because silence means something different for
//!   one (see [`TurnInput::EchoDeadline`]).
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
//! restored marker set** ([`SendStore::restore_all_dispatched`]) before
//! any session actor exists, so rebuilt-Idle turn state and the store agree
//! that nothing is outstanding. The restored row does not re-dispatch on its
//! own — the message may be stale by the time the session reopens — it stays
//! visible in the open-send list until the user explicitly releases it into
//! the normal queued flow ([`SendStore::release_held_send`]) or
//! cancels it.
//!
//! [`SendStore::restore_all_dispatched`]: crate::ports::SendStore::restore_all_dispatched
//! [`SendStore::release_held_send`]: crate::ports::SendStore::release_held_send
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
//!   parks it once the budget is spent — the row stays `queued` but stops
//!   dispatching on its own, waiting for the user to release or cancel it, so
//!   the bound costs no message either. The count is history rather than turn
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
//!   correlation. The echo deadline of a slash-command send settles the same
//!   way: a local command that ran leaves no trace at all, so silence is its
//!   delivery (see [`TurnInput::EchoDeadline`]).

mod orphaned_send;
pub use orphaned_send::OrphanedSend;

mod transition;
pub use transition::{transition, Transition};

mod turn_input;
pub use turn_input::TurnInput;

mod turn_input_for_agent_event;
pub use turn_input_for_agent_event::turn_input_for_agent_event;

mod turn_state;
pub use turn_state::TurnState;
