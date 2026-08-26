---
status: completed
pipeline_phase: null
plan: null
base_ref: feat/attribution-split
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && U=backend/crates/domain/delta-usecase/src && grep -q "SettleIfUnmatched" $U/turn.rs && ! grep -rq "CancelIfUnmatched" backend/crates && grep -rq "fn settle_send_delivered" backend/crates/gateway/delta-sqlite/src/store && grep -rq "fn settle_send_delivered" $U/ports'
assignee: null
branch: task/0825-1542-feat-position-based-send-consumption
created_at: 2026-08-25T15:42:55Z
updated_at: 2026-08-25T16:45:53Z
---

# feat(usecase): consume the outstanding send by position, not by echo text

## Overview

A Claude Code session gives Delta no round-trippable id for a prompt it typed
into the pane. `UserPromptSubmit` carries only `prompt`, `session_id`,
`transcript_path` and `cwd` (`backend/crates/domain/delta-usecase/src/ports/user_prompt_submit_hook.rs`),
so Delta correlates a dispatched send with its echo by **text equality**:
`interactor/hooks/on_user_prompt_submit.rs` compares
`head_dispatched_send().text` with the hook prompt through
`claude_format::prompt_echoes_send` and feeds the answer into the turn state
machine as `TurnInput::EchoMatched { send_id }` (equal) or
`TurnInput::ExternalPrompt` (not equal). Every time Claude rewrites the prompt
between typing and recording — local-command folding, the unknown-command
notice, namespace expansion, the `[Image #N]` prefix — the comparison fails,
`(AwaitingEcho, ExternalPrompt)` in `turn.rs` orphans the send with
`OrphanedSend::Requeue`, and the same text is typed again: one more model turn
spent delivering a message that was already delivered. The requeue budget
(`MAX_REQUEUES_PER_SEND = 1` in `session_actor/runtime/turn.rs`) and the
`EchoDeadline` watchdog bound the damage but do not remove the cause.

The cause is that one text comparison decides two unrelated things: *whether
the dispatched send's turn has started* and *which thread its transcript lines
belong to*. This task separates the first from the text. The observation that
makes it safe: while the machine is in `AwaitingEcho { send_id }`, that send's
keystrokes **have already been typed into the pane** (or are held for a
resuming pane — see the resume guard below). Under the single-outstanding rule
there is exactly one outstanding send, so a `UserPromptSubmit` arriving in
that state is the outcome of that send regardless of what text the hook
reports. Treating it as such makes the worst case "delivered once, attributed
to the wrong thread" instead of "delivered twice" or "stuck as In Progress".
Codex already works this way — `dispatch_agent_turn` in
`interactor/lifecycle/spawn_adapter_session.rs` marks the send matched from the
provider's turn id and never consults the text — so this brings the Claude
path to parity with the Codex path. Thread attribution keeps using the text
(it lives in the `delta-attribution` crate and is a separate change); this
task only changes what the turn machine and the send row do.

### What changes

1. **`on_user_prompt_submit.rs` decides by position.** With an outstanding
   dispatched send and the runtime not in its resume window
   (`SessionRuntime::is_resuming()`, already used by the echo-deadline sweep for
   the same reason), emit `TurnInput::EchoMatched { send_id }` whether or not
   `prompt_echoes_send` holds. Emit `TurnInput::ExternalPrompt` only when there
   is no outstanding send, or the runtime is resuming (the held keystrokes have
   not been typed yet, so a prompt arriving now cannot be theirs). Keep
   `prompt_echoes_send` available to the parts of this handler that still need
   it (the `ExternalInput` event and the `thread_switch_context` locator quote)
   — decide from the code whether `ExternalInput` should still fire on a
   text mismatch while the send is consumed, and document the choice in the
   handler. The `(AwaitingEcho, ExternalPrompt)` arm of `turn.rs` becomes
   unreachable from this handler; keep it as a defensive arm and say so in its
   comment.
2. **A consumed-but-unattributed send settles as delivered, not cancelled.**
   Today `InFlight { send_id: Some(n) }` + `Stop` / `Interrupt` / `Close` /
   `DispatchFailed` yields `OrphanedSend::CancelIfUnmatched(n)`, which cancels a
   send whose transcript line never text-matched — i.e. a delivered message is
   recorded as undelivered. Replace `CancelIfUnmatched` with
   `OrphanedSend::SettleIfUnmatched(n)` (everywhere; no arm keeps the old
   disposition), and have `interactor/turn_input.rs` execute it through a new
   `SessionStore::settle_send_delivered(id) -> Result<bool>` implemented in
   `delta-sqlite/src/store/sends.rs` as
   `UPDATE send SET status = 'matched' WHERE id = ?1 AND status = 'dispatched'`
   (returning whether a row changed). No schema change: `matched_uuid` is
   nullable and independent of the `status` CHECK constraint, and the web UI
   never reads `matched_uuid`. Update the `fake_store` used by usecase tests.
3. **The requeue budget and the `EchoDeadline` watchdog stay.** After this
   change `OrphanedSend::Requeue` is produced only by `EchoDeadline` and by
   defensive arms; the budget of one becomes "one retry for a send nobody heard
   about". Update the doc comments on `OrphanedSend::Requeue`, `MAX_REQUEUES_PER_SEND`
   and the module docs of `turn.rs` so they describe the new division of labour
   (position decides consumption; text decides attribution) instead of the old
   mismatch semantics.
4. **Tests.** The golden transition table `the_transition_table_is_exactly_this`
   in `turn.rs` changes for every `CancelIfUnmatched` row. In
   `interactor/hooks/tests/`, `unmatched_prompt_is_external_input` and in
   `interactor/enqueue/tests/`, `unmatchable_send_is_redispatched_at_most_once`
   (rename it to `unmatchable_send_is_never_redispatched`)
   and `outstanding_send_matches_and_marks_send` pin the old behaviour — rewrite
   them to pin the new one (a mismatched echo consumes the send and is never
   redispatched). Add tests for: a mismatched `UserPromptSubmit` while
   `AwaitingEcho` moves to `InFlight { Some(send_id) }` with no orphan; a prompt
   during the resume window does not consume the held send; a turn ending on a
   consumed-but-unattributed send leaves the row `matched`, not `cancelled`;
   `settle_send_delivered` is a no-op on a row that is already `matched` or
   `cancelled`. Keep `swallowed_send_is_retyped_then_parked_by_the_echo_deadline`
   and `codex_turn_completing_does_not_cancel_its_send` green as they are —
   they describe behaviour this task must preserve.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `OrphanedSend::SettleIfUnmatched` exists in `delta-usecase/src/turn.rs` and
      `CancelIfUnmatched` no longer appears anywhere under `backend/crates`
      (gates appended to `check_command`).
- [x] `SessionStore::settle_send_delivered` is declared in
      `delta-usecase/src/ports` and implemented in
      `delta-sqlite/src/store/sends.rs` with a `status = 'dispatched'` guard
      (gates appended to `check_command`; the guard is pinned by the new
      no-op test).
- [x] A `UserPromptSubmit` whose text does not equal the outstanding send still
      drives `AwaitingEcho { n }` → `InFlight { Some(n) }` and the send is never
      requeued — pinned by `unmatchable_send_is_never_redispatched` (the rewrite of the
      former `unmatchable_send_is_redispatched_at_most_once`, now asserting
      zero redispatches) and the new mismatch test.
- [x] A `UserPromptSubmit` during the resume window does not consume the held
      send — pinned by a new test.
- [x] The existing echo-deadline recovery (`swallowed_send_is_retyped_then_parked_by_the_echo_deadline`)
      and the Codex parity test (`codex_turn_completing_does_not_cancel_its_send`)
      pass unchanged.
- [x] `make check` is green (backend fmt / build / test / clippy `-D warnings`,
      generated-bindings freshness, frontend, both Playwright suites).

### Manual / on-hardware (verified by a human before merge)

- [x] In a real Claude Code session, make the `UserPromptSubmit` prompt differ
      from the send's text and confirm the send is delivered exactly once, leaves
      the open list, and no `SendParked` notice appears. Verified 2026-08-26 by
      typing extra characters into the tmux pane in the 250 ms gap between
      Delta's paste and its Enter (a script polling `capture-pane` for the pasted
      text): the prompt arrived with the extra text, the server logged the
      "does not equal the outstanding send's text" line once, the message was
      typed once, no external-input or parked notice appeared, and after the
      turn the row was `matched` with `matched_uuid` NULL ("turn ended with its
      send unattributed" logged once).

## Out of scope

- Thread attribution (`delta-attribution`: `thread_resolution.rs`, corpus
  goldens) — a follow-up task on the same integration branch changes how a
  mismatched transcript line is attributed; this task must not touch that
  crate.
- The local-command / unknown-command name matching in `delta-attribution`.
- Merging `EchoMatched` and `ExternalPrompt` into one input. Note in the PR
  body whether the refined code would read better with a single
  `PromptSubmitted { send_id: Option<i64> }` input, but do not do it here.
- Any change to the Codex adapter path or to the web frontend.
