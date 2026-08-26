---
status: completed
pipeline_phase: null
plan: null
base_ref: feat/attribution-split
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && A=backend/crates/domain/delta-attribution && grep -q "attributed" $A/src/attribute/effect.rs && ! grep -rq "resets_carry_to_main_without_consuming" $A/tests && grep -q "prompt_echoes_send" $A/src/attribute/thread_resolution.rs'
assignee: null
branch: task/0826-1200-feat-attribute-consumed-send-by-position
created_at: 2026-08-26T12:00:56Z
updated_at: 2026-08-26T12:49:28Z
---

# feat(attribution): attribute the consumed send's echo line by position, not by text

## Overview

The turn machine now consumes a dispatched send by position: while one send is
outstanding, the next `UserPromptSubmit` is that send's echo whatever text the
hook reports (`interactor/hooks/on_user_prompt_submit.rs`), and a send whose
echo never text-matched settles `matched` at turn end
(`OrphanedSend::SettleIfUnmatched` → `SessionStore::settle_send_delivered`).
Transcript attribution still decides by text. In
`backend/crates/domain/delta-attribution/src/attribute/thread_resolution.rs`,
the human-turn branch compares the head `OutstandingSend.text` with the
transcript user line through `claude_format::prompt_echoes_send`; on a match it
pops the send, emits `Effect::SendMatched { send_id, matched_uuid }` and
attributes the line (and everything that follows it) to `send.thread_id` with
`send.semantic_parent_uuid`; on a mismatch it leaves the send outstanding,
resets `carry_thread` to `main_thread` and lands the line on main.

That mismatch branch is now the wrong outcome. Seen on a real session: a send
to a branch thread whose prompt Claude Code received with extra characters was
delivered exactly once (the positional consumption worked), but the user line
and the assistant reply were attributed to the main thread — from the branch
thread's transcript pane they simply vanished. Under the single-outstanding
rule the first human user line after a dispatch *is* that send's echo, by the
same positional argument the turn machine already relies on, so the line
belongs on the send's thread regardless of what the text looks like. The text
comparison keeps one job: saying whether the echo was recognised verbatim, so
a rewrite can be logged.

### What changes

1. **`thread_resolution.rs`, human-turn branch.** If `state.outstanding` has a
   head, pop it unconditionally: emit `Effect::SendMatched` for it with the
   line's uuid, set `carry_thread = send.thread_id`, and attribute the line to
   `(send.thread_id, send.semantic_parent_uuid)` — exactly what the match arm
   does today. Compute `attributed = prompt_echoes_send(&send.text, trimmed)`
   and carry it on the effect (see 2); it no longer gates anything. The
   `None if is_queued_command` and `None` arms stay as they are for the case
   with no outstanding send. Update the branch's comment and the
   `OutstandingSend.text` doc (`outstanding_send.rs`: "the text the echo is
   recognized by" → the text the echo is *compared against for the
   `attributed` flag*).
2. **`Effect::SendMatched` gains `attributed: bool`** (`attribute/effect.rs`).
   The consumer in `interactor/sync/sync_transcript.rs` still calls
   `mark_send_matched(send_id, &matched_uuid)`; when `attributed` is false it
   logs one `tracing::warn!` naming the send id and both texts, so a new
   Claude Code rewrite is visible in the server log the first time it
   happens. No new event to the browser (that is a follow-up).
3. **Full-history replay.** `delta-attribution/src/replay.rs` folds a whole
   transcript with every dispatched send seeded into `state.outstanding`
   (FIFO). Read it and decide whether positional consumption is correct there
   too — the concern is an external human line mid-history eating the head
   send that actually belongs to a later line. If replay needs a guard (e.g.
   only consume when the line is at or after the send's dispatch position),
   implement the smallest one that keeps the corpus honest; if the corpus
   shows no such case, say so in the PR body and keep replay identical to the
   live fold. Do not leave this undecided.
4. **Corpus goldens.** `tests/corpus/cases/*/expected.json` carry both
   assignments and effects. Regenerate with `UPDATE_GOLDEN=1 cargo test -p
   delta-attribution --test corpus`, then **review every changed golden by
   hand** and list them in the PR body with one line each saying why the new
   assignment is right (expected to move: `external_input_only`,
   `unmatched_queued_command`, `local_command_no_turn`, possibly
   `multi_send_session`). A golden that changes for a reason you cannot
   explain is a bug in this change, not a golden to accept.
5. **Tests.** In `tests/fold.rs`, replace
   `an_external_human_line_resets_carry_to_main_without_consuming_the_send`
   with a test of the new rule (a human line whose text differs from the
   outstanding send still consumes it, lands on the send's thread, and emits
   `SendMatched { attributed: false }`); keep
   `send_matching_compares_trimmed_text` but make it assert the `attributed`
   flag rather than consumption; add a test that with **no** outstanding send
   a human line still resets carry to main (the behaviour that remains). Add
   a usecase test under `interactor/sync/tests/` (one test per file, declared
   in that directory's `mod.rs`) that a mismatched transcript line for a
   branch send attributes the user line and the following assistant line to
   the child thread and marks the send `matched` with that line's uuid. Check
   `interactor/enqueue/tests/turn_end_settles_a_consumed_send_as_delivered`
   still describes a real path (a consumed send whose transcript never shows
   a human line — e.g. the turn ends before ingestion) and adjust its setup
   if it relied on the mismatch leaving the row `dispatched`.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `Effect::SendMatched` carries `attributed: bool` and
      `thread_resolution.rs` still calls `prompt_echoes_send` (only to compute
      that flag) — gates appended to `check_command`.
- [x] The old fold test
      `an_external_human_line_resets_carry_to_main_without_consuming_the_send`
      is gone (gate appended to `check_command`) and its replacement pins:
      mismatched text → send consumed, line on the send's thread,
      `attributed: false`.
- [x] A human line with no outstanding send still resets carry to main —
      pinned by a fold test.
- [x] A branch send whose transcript line text differs is attributed to the
      child thread together with the following assistant line, and the send
      row is `matched` with that line's uuid — pinned by the new usecase test.
- [x] Every corpus case replays to its golden (`every_corpus_case_replays_to_its_golden_assignments`)
      after the regeneration, and the batch-split invariance suite
      (`tests/replay_properties.rs`) is green.
- [x] `make check` is green (backend fmt / build / test / clippy `-D warnings`,
      generated-bindings freshness, frontend, both Playwright suites).

### Manual / on-hardware (verified by a human before merge)

- [x] On a real Claude Code session, send to a **branch** thread and make the
      hook prompt differ from the send text (type extra characters into the
      tmux pane in the 250 ms gap between Delta's paste and its Enter — a
      script polling `tmux capture-pane` for the pasted text does it
      reliably). The user line and the assistant reply appear in the branch
      thread's pane, the server logs one `attributed=false` warning, and the
      send row is `matched` with the line's uuid. Verified 2026-08-26: the
      rewritten user line and the reply were attributed to the branch thread
      (not main), the server logged the hook-side "does not equal" info line
      and the ingest-side "still attributed to the send's thread" warning
      once each, and the row was `matched` with `matched_uuid` equal to the
      line's uuid.

## Out of scope

- The local-command and unknown-command branches of `thread_resolution.rs`
  (still text-based; a follow-up on this integration branch).
- Any browser-facing notice for an unattributed echo, and merging
  `TurnInput::EchoMatched` / `ExternalPrompt` (follow-ups).
- The turn machine (`delta-usecase/src/turn.rs`), the Codex adapter, and the
  frontend.
