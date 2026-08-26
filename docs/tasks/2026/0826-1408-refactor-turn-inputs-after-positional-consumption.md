---
status: completed
pipeline_phase: null
plan: null
base_ref: feat/attribution-split
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && U=backend/crates/domain/delta-usecase/src && grep -q "PromptSubmitted" $U/turn.rs && grep -q "CommandResolved" $U/turn.rs && ! grep -rqE "EchoMatched|ExternalPrompt" backend/crates && grep -rq "fn drop_held_prompt" $U/interactor/session_actor/runtime'
assignee: null
branch: task/0826-1408-refactor-turn-inputs-after-positional-consumption
created_at: 2026-08-26T14:08:35Z
updated_at: 2026-08-26T14:53:07Z
---

# refactor(usecase): give the turn machine inputs that say what happened, now that consumption is positional

## Overview

Three changes on this branch moved send consumption and attribution from text
equality to position: the `UserPromptSubmit` hook consumes the outstanding send
whatever its text, the transcript fold attributes the echo line to the send's
thread by position, and a slash-command send is resolved by its command line
by position. The turn state machine in
`backend/crates/domain/delta-usecase/src/turn.rs` still speaks the old
vocabulary, and two of its paths now misdescribe what happens:

1. **`TurnInput::EchoMatched { send_id }` and `TurnInput::ExternalPrompt` differ
   only by an `Option`.** `interactor/hooks/on_user_prompt_submit.rs` computes
   exactly `consumed: Option<send_id>` and picks one of the two; the names still
   carry the "text matched / did not" meaning the code no longer honours, and
   `(AwaitingEcho, ExternalPrompt)` exists only to describe "a prompt arrived
   with a send outstanding but held for a resuming pane". Merge them into one
   input, `TurnInput::PromptSubmitted { send_id: Option<i64> }`. The Codex path
   (`interactor/lifecycle/spawn_adapter_session.rs`) feeds `ExternalPrompt`
   today and becomes `PromptSubmitted { send_id: None }`; its turn-end test
   (`codex_external_prompt_turn_end_orphans_nothing_unlike_claude`) keeps its
   meaning under the new name.
2. **A slash-command send's turn end reaches the machine as `Stop`.**
   `interactor/sync/sync_transcript.rs` maps `Effect::LocalCommandTurnEnded`
   to `apply_turn_end(Completed)`, i.e. `TurnInput::Stop`, while the machine
   is still in `AwaitingEcho { send_id }` (a local or unknown command fires no
   `UserPromptSubmit`). That lands on the defensive `(AwaitingEcho, Stop)` arm:
   the machine logs an "anomalous turn transition" warning, hands back
   `OrphanedSend::Requeue`, and `turn_input.rs` spends one unit of the requeue
   budget on a `requeue_send` that is a no-op only because the
   `SendMatched` effect just before it already moved the row to `matched`.
   Seen on a real session for every resolved slash command: two misleading
   warnings per command. Give this outcome its own input,
   `TurnInput::CommandResolved { send_id }`, carried on the effect
   (`Effect::LocalCommandTurnEnded { send_id }` in the `delta-attribution`
   crate — the send id is already known where the effect is emitted).
   `(AwaitingEcho { n }, CommandResolved { n })` → `Idle`, no orphan, not
   anomalous; a mismatched id keeps the outstanding send and is anomalous;
   from `Idle` / `InFlight` it is anomalous and converges on `Idle` with the
   same disposition the corresponding `Stop` arm uses. `turn_input.rs` must not
   touch the requeue budget on this input.
3. **A held first prompt can be typed twice.** During the resume window a send
   row is written and the machine is `AwaitingEcho`, but the keystrokes are
   held on `ResumingSession::held_prompt`
   (`interactor/session_actor/runtime/spawn.rs`). If a prompt arrives before
   settle, `(AwaitingEcho, PromptSubmitted { send_id: None })` requeues the
   send — but `held_prompt` still carries its text, so
   `interactor/lifecycle/dispatch_ready_resumes.rs` types the held text at
   settle *and* the next idle flush dispatches the `queued` row: the message is
   delivered twice. Fix: when the requeue disposition fires while
   `is_resuming()`, drop the held prompt (add
   `SessionRuntime::drop_held_prompt()` next to `hold_first_prompt`) so settle
   takes the "no held first prompt; flushing any queued send" branch and the
   row is typed exactly once. With that, `(AwaitingEcho, PromptSubmitted { None })`
   is a designed-for outcome (the resume window) and should no longer be
   flagged anomalous — the module docs on `TurnInput::EchoDeadline` state the
   convention (designed-for outcomes are not anomalous).

Update the golden transition table (`the_transition_table_is_exactly_this`:
4 states × 9 inputs after the merge and the addition), the module docs of
`turn.rs` and `turn_input.rs`, and every doc comment that names the removed
inputs (`grep -rn "EchoMatched\|ExternalPrompt" backend/crates docs/guides`).
No wire or frontend change: `WireTurn.state` still reads `awaiting_echo` /
`in_flight`, and no event shape changes.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `TurnInput::PromptSubmitted { send_id: Option<i64> }` and
      `TurnInput::CommandResolved { send_id }` exist, and `EchoMatched` /
      `ExternalPrompt` no longer appear anywhere under `backend/crates`
      (gates appended to `check_command`).
- [x] `(AwaitingEcho { n }, CommandResolved { n })` reaches `Idle` with no
      orphan and `anomalous == false` — pinned in the golden table and by a
      usecase test that resolves an unknown-command send and asserts no
      requeue was claimed and no anomalous transition was logged (assert on the
      runtime's requeue count and turn state; the existing
      `unknown_command_unsticks_turn` / `local_command_unsticks_turn_and_folds_to_meta`
      tests may be extended for this).
- [x] A held first prompt whose send is requeued inside the resume window is
      typed exactly once after settle — pinned by a new enqueue/lifecycle test
      (`SessionRuntime::drop_held_prompt` exists; gate appended to
      `check_command`).
- [x] `(AwaitingEcho, PromptSubmitted { send_id: None })` is `anomalous == false`
      in the golden table.
- [x] `make check` is green (backend fmt / build / test / clippy `-D warnings`,
      generated-bindings freshness, frontend, both Playwright suites).

### Manual / on-hardware (verified by a human before merge)

- [ ] On a real Claude Code session, send an unknown slash command from Delta
      (e.g. `/nosuchcommand`): the send clears from the open list, and the
      server log shows **no** "anomalous turn transition" and **no**
      "outstanding send never echoed" warning for it.

## Out of scope

- Any browser-facing notice for a rewritten echo, and making a parked send
  recoverable (a separate change on this branch that touches the wire and
  the frontend).
- The Codex adapter's behaviour and the frontend.
