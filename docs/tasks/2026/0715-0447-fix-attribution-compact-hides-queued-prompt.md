---
status: completed
pipeline_phase: null
plan: null
base_ref: null
retries_remaining: 1
check_command: "cd backend && cargo build && cargo test && cargo clippy --all-targets -- -D warnings"
assignee: null
branch: task/0715-0447-fix-attribution-compact-hides-queued-prompt
created_at: 2026-07-15T04:47:25Z
updated_at: 2026-07-15T06:03:08Z
---

# fix(attribution): stop folding post-compact queued user prompts as local-command machinery

## Overview

When Claude Code auto-runs (or the user manually runs) `/compact` while a user
prompt is sitting in the CLI's internal input queue, the prompt is replayed
after compact as a plain `type: "user"` line carrying `promptSource: "queued"`.
Claude Code assigns that replay the SAME `promptId` as the `/compact`
local-command group, because both belong to the one `promptId` the CLI's
post-compact turn opens with. Delta's attribution fold then swallows the
replay as command machinery: it never renders in the conversation pane, the
send row is silently marked matched, and the turn is short-circuited to
`TurnInterrupted` while the actual assistant response for the prompt streams
in against a torn-down turn.

The sequence in the recorded transcript is:

- Line 1345/1346: `queue-operation` enqueue/dequeue records — the CLI accepted
  the user's prompt A mid-turn (turn was the auto-compact) and queued it.
- Line 1347/1348: `compact_boundary` + the `isCompactSummary:true` summary
  line, both stamped with `promptId 6ef9300c-d188-432c-92aa-cdb99080b777`.
- Line 1349: `<local-command-caveat>` (`isMeta:true`), same `promptId`.
- Line 1350: `<command-name>/compact</command-name>`, same `promptId`.
- Line 1351: `<local-command-stdout>Compacted…</local-command-stdout>`, same
  `promptId`.
- Line 1360: the queued replay — a plain `type: "user"` line whose
  `message.content` is the human's actual prompt A, `promptSource: "queued"`,
  SAME `promptId 6ef9300c-…`.
- Lines 1371/1373/1377: tool_result blocks from A's assistant response, still
  the same `promptId`.

Delta's fold in
`backend/crates/domain/delta-attribution/src/attribute.rs` handles this
`promptId` as a local-command group:

- The caveat branch (`attribute.rs:339-345`) records the caveat's `promptId`
  in `state.local_command_prompts`.
- The reclassify branch (`attribute.rs:353-361`) folds any subsequent
  `Role::User` line sharing that `promptId` to `Role::Meta` and computes
  `is_local_command_name_line = true` for it.
- The name-line branch (`attribute.rs:533-560`) compares the folded line's
  trimmed text against the head outstanding send's text — a bare send text
  and a bare queued replay text are equal — so it emits `SendMatched` +
  `LocalCommandTurnEnded` against the queued replay.
- The final `messages.push(Message { … role, … })` at `attribute.rs:655-676`
  persists the queued replay with `role: Role::Meta`, so the frontend renders
  it as collapsed command machinery instead of a user bubble.

Observed by the user: the prompt A disappears from delta's conversation
pane; the pending "typing…" chip clears; Claude Code's response to A streams
in normally, but delta's turn state receives a `TurnInterrupted` event as if
the send had been aborted.

**Fix direction — recognize the modern queued replay in the parser and
exclude it from local-command grouping in the fold.** The parser
(`backend/crates/gateway/delta-transcript/src/parse/mod.rs`) currently only
lifts the LEGACY `queued_command` attachment shape into
`TranscriptMessage.is_queued_command`; the modern `promptSource: "queued"`
on a plain user line is not surfaced at all (see the passing test
`dequeued_user_line_parses_as_a_plain_user_message`, which explicitly asserts
`is_queued_command == false` for it — that test must be revisited as part of
this fix).

Specifically:

1. In `raw_line.rs`, add a `promptSource: Option<String>` field (existing
   struct is the right place — it already carries `prompt_id`,
   `is_compact_summary`, `is_meta`, `is_api_error_message`).
2. In `TranscriptMessage`
   (`backend/crates/domain/delta-usecase/src/interactor/…` — the shared type
   the parser emits), add a boolean field alongside the existing
   `is_queued_command`. Suggested name: `is_queued_replay` (kept distinct so
   downstream code does not confuse it with the legacy attachment; do not
   overload `is_queued_command`, whose semantics `attribute.rs:578-590`
   already relies on).
3. In `parse/mod.rs`, set `is_queued_replay = raw.prompt_source.as_deref() == Some("queued")`
   on the emitted `TranscriptMessage`. This is independent of
   `is_queued_command` (they can both be true in principle for an older CLI
   version, though in practice the two shapes never co-occur on one line).
4. In `attribute.rs`, at the reclassify branch
   (`in_local_command_group` computation), exclude lines with
   `is_queued_replay == true` from being folded to `Role::Meta`:

       let in_local_command_group = !line.is_queued_replay
           && line
               .prompt_id
               .as_ref()
               .is_some_and(|id| state.local_command_prompts.contains(id));

   Rationale (worth a short comment in the code): Claude Code re-emits a
   queued prompt post-compact under the compact group's `promptId`. The
   replay is a genuine human turn, not command machinery, so it must NOT
   inherit the group's Meta reclassification. A queued replay is also NOT a
   `<command-name>` line, so leaving it out of the group means it flows
   through the normal `is_human_turn` branch:
   `head_matches → SendMatched → attribute to send's thread`, exactly as if
   no compact had happened. The `Effect::AutoCompactFinished` emitted by the
   summary line still drives the existing debounced re-dispatch path — that
   path is untouched.

The legacy `queued_command` attachment path
(`attribute.rs:578-590` — the `None if line.is_queued_command =>` arm) is
independent and stays as-is. It handles the older-CLI shape where the queued
prompt is an attachment carrying its text; that shape does not participate
in local-command grouping (no `promptId` collision) and the arm's
`inherit-carry-thread` behavior is correct for it.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] Parser: a `type: "user"` line carrying `promptSource: "queued"` sets
      `is_queued_replay = true` on the emitted `TranscriptMessage`; an
      ordinary user line (no `promptSource`, or `promptSource: "cli"` /
      other) leaves it `false`. The existing
      `dequeued_user_line_parses_as_a_plain_user_message` test is updated to
      assert the new flag is `true` (keeping its `is_queued_command ==
      false` assertion — the two fields are distinct).
- [x] Attribution: a corpus/unit test ingests the exact seven-line
      compact-then-queued-replay shape observed in
      `019f5f23-dae2-73b3-a1d9-07d61263c053.jsonl` — compact summary,
      caveat, command-name, stdout, then a queued replay whose text equals
      an outstanding `Dispatched` send, all sharing one `promptId`. The
      replay attributes to the send's thread as `Role::User` (NOT
      `Role::Meta`), emits `Effect::SendMatched` for the outstanding send
      (NOT `Effect::LocalCommandTurnEnded`), and — because the summary was
      also folded — emits `Effect::AutoCompactFinished` exactly once. A
      regression assertion pins that the replay's persisted `Message.role`
      is `Role::User`.
- [x] Attribution: same fixture with NO outstanding send (the queued
      replay lands cold) — the replay still folds as `Role::User`, hits the
      `None => (main_thread, …)` external-input arm, and lands on `main`
      (never `Role::Meta`). No `LocalCommandTurnEnded` emitted.
- [x] Attribution: existing local-command tests still pass — a real
      `/review-pr`-style group (caveat → command-name equal to an
      outstanding send → stdout, no queued replay) still emits
      `SendMatched` + `LocalCommandTurnEnded` on the command-name line and
      folds the group to `Role::Meta`. Name the specific existing test in
      the diff so the reviewer can see it is exercised, not just untouched.
- [x] Attribution: existing
      `a_compact_summary_line_inherits_carry_and_does_not_consume_the_outstanding_send`
      (or whatever the current name is; the regression cited in
      `attribute.rs:527`) still passes — the compact summary itself is not
      matched against the outstanding send.
- [x] `sync_transcript` integration test: given the same seven-line
      fixture, `poll_transcript` upserts the queued replay as a
      `Role::User` message on the send's thread, marks the send row
      `matched` via `mark_send_matched`, and does NOT emit
      `SessionEvent::TurnInterrupted`. The existing
      `compact_summary_redispatches_stuck_dispatched` /
      `compact_summary_redispatch_is_debounced_against_hook` tests still
      pass unmodified.
- [x] `cd backend && cargo build && cargo test && cargo clippy --all-targets -- -D warnings`
      is green (the task's `check_command`).

### Manual / on-hardware (verified by a human before merge)

- [ ] Live repro of the exact scenario: submit a prompt in delta while the
      CLI is in the middle of an auto- or manual `/compact` on a
      near-full-context session. After compact completes and Claude Code's
      reply streams in, the prompt is visible in the conversation pane as a
      user bubble on the expected thread, the pending "typing…" chip
      clears normally (no lingering send, no `TurnInterrupted` toast), and
      the reply attaches under the prompt (not orphaned).
- [ ] Regression check: a plain slash-command run against an idle session
      (`/review-pr` or any local command delta dispatches as a send) still
      renders collapsed as command machinery and its pending chip clears —
      no user bubble for the command-name line.

## Out of scope

- Closing a local-command group by observing the trailing
  `<local-command-stdout>` line (alternative fix direction 2 discussed
  during triage). Not needed once queued replays are excluded upfront, and
  it opens a batch-boundary hazard the current design avoids.
- Content-pattern-based tightening of `is_local_command_name_line` to
  `<command-name>…` shape (alternative fix direction 3). More brittle
  against Claude Code format drift; the parser-side flag is more principled.
- Any change to the `AutoCompactFinished` re-dispatch path or its debounce.
- Any change to the legacy `queued_command` attachment path
  (`attribute.rs:578-590`).
- Frontend rendering changes. Restoring the correct `Role::User` at
  attribution is sufficient; the existing pane rendering already handles
  User rows.
