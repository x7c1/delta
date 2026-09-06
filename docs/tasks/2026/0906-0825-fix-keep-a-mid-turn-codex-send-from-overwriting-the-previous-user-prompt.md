---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && ! grep -rq "codex-turn-" backend/crates/gateway/codex-agent/src/ && grep -rq "two_prompts_accepted_under_one_turn_id_get_distinct_uuids_and_keep_their_own_lane" backend/crates/gateway/codex-agent/src/ && grep -rq "a_prompt_accepted_before_its_turn_started_is_restamped_with_that_turn" backend/crates/gateway/codex-agent/src/ && grep -rq "a_prompt_steered_into_a_running_turn_keeps_that_turn_and_is_not_restamped_by_the_next" backend/crates/gateway/codex-agent/src/ && [ "$(grep -rc "#\[test\]" backend/crates/gateway/codex-agent/src/ | awk -F: "{s+=\$2} END {print s}")" -ge 122 ]'
assignee: null
branch: task/0906-0825-fix-keep-a-mid-turn-codex-send-from-overwriting-the-previous-user-prompt
created_at: 2026-09-06T08:25:00Z
updated_at: 2026-09-06T10:32:17Z
---

# fix(codex): keep a mid-turn send from overwriting the previous user prompt

## Overview

In a Codex session, a prompt sent while a turn is still running can **erase
the previous prompt from the conversation and land on the wrong thread**. The
user-visible shape (dogfooding, 2026-09-06): the user branched from a passage
of the assistant's reply (branch send → new child thread) while the turn was
still working; the branch thread opened, the assistant's answer landed on it,
but the user's own question was shown on the *parent* thread instead — and
the plain prompt sent a few minutes earlier had disappeared from the parent
thread entirely. Claude sessions are unaffected: their user prompts carry the
transcript's own uuids.

### What happens

The Codex adapter emits `AgentEvent::UserPromptAccepted` **before** it issues
`turn/start` (`backend/crates/gateway/codex-agent/src/adapter/mod.rs`,
`fn start_turn`: "so it always precedes the turn's pushed notifications"). The
content fold (`backend/crates/gateway/codex-agent/src/content.rs`) then mints
the prompt's uuid in `fn user_prompt_uuid` from `self.current_turn` — the id
`AgentEvent::TurnStarted` last recorded — as `codex-turn-<id>-user`, on the
assumption "one user prompt per turn". But at that moment `current_turn` is
never *this* prompt's turn: the new turn's `turn/started` has not arrived yet,
so it is the **previous** turn's id (observed on every user row of the
affected session: the prompt of send N carries the turn id that send N-1's
`turn/start` returned), or, for a prompt sent mid-turn, the **running**
turn's id.

Codex accepts `turn/start` while a turn is in flight and steers the input into
the running turn: `turn/start` answers with a fresh turn id (which Delta
records as the `send` row's `matched_uuid`), but no `turn/started` follows and
every later item keeps the running turn's `turnId`. The adapter dispatch path
(`interactor/enqueue/enqueue_send.rs`, the `open_agent()` branch →
`dispatch_agent_turn`) has no mid-turn gate, so two prompts accepted under one
running turn are folded with the **same uuid**. The persistence upsert
(`backend/crates/gateway/delta-sqlite/src/store/messages.rs`,
`upsert_messages`) then replaces the first prompt's content/seq/created_at
with the second's, while deliberately **not** touching `thread_id` /
`semantic_parent_uuid` (they are the thread overlay, authoritative on first
ingest — that policy is right for Claude re-ingest and must stay). Net effect:
the earlier prompt is gone, and the later prompt keeps the earlier one's
thread and (absent) semantic parent — exactly the reported symptom.

Evidence from the affected session (`01a0758a-d920-7ba1-ba98-fdbe17372d18`,
`provider = codex`; `send` and `message` rows): send 643 (plain, thread 196)
started turn `01a07598-4854…`; send 644 (plain, thread 196, 07:22:27) and send
645 (branch → thread 197, 07:27:36, `semantic_parent` set) were both dispatched
while that turn ran and got `matched_uuid`s `…-df10…` / `…-92b5…` that never
appeared as `turn/started`. Only one user row exists for the two:
`codex-turn-01a07598-4854…-user`, `thread_id = 196`, `semantic_parent_uuid =
NULL`, content = send 645's text, `seq = 123` — while every item after it
(`seq` 124–158, `prompt_id = 01a07598-4854…`) sits on thread 197 as the branch
routing (`begin_turn`) intended. The unit test
`a_turn_id_becomes_the_prompt_group_and_seq_is_monotonic` encodes the
ordering the live adapter never produces (`TurnStarted` *before*
`UserPromptAccepted`), which is why this was not caught.

### Fix — one PR, two parts of the same defect

**Part A — a user prompt's uuid must not depend on the turn id.** In
`content.rs`, make `user_prompt_uuid` always mint `codex-user-<seq>` (the
existing fallback: `seq` is monotonic per session and re-seeded from the
store's `MAX(seq) + 1` on resume, so it is unique across the whole session —
see `a_resumed_sources_first_prompt_does_not_collide_with_the_pre_restart_one`).
Delete the `codex-turn-<id>-user` branch and every mention of it. The module
docs and the method's doc comment must state the real facts: `current_turn`
at `UserPromptAccepted` time is the previous or the running turn, never the
prompt's own, and one turn can accept several prompts (Codex steering), so
keying off the turn id collides and the upsert then silently drops a prompt
and pins the survivor to the first prompt's lane.

**Part B — give the prompt its own turn's `prompt_id`, or none.** Today the
prompt's `prompt_id` is the previous turn's id for the same reason (visible on
every user row of the session). Two small changes in `content.rs`:

- clear `current_turn` on `TurnCompleted` (after `flush_pending_tools`, which
  still stamps the closing turn on late tool messages), so a prompt accepted
  while idle degrades to `prompt_id = None` instead of inventing the previous
  turn's id — "degrade, never fake" is the crate's stated contract;
- remember the prompt just built as the turn's pending root (an
  `Option<Message>`, `Message` is `Clone`), and when the next `TurnStarted`
  carries an id, re-emit that same message with `prompt_id` set to it (same
  uuid, same seq, same thread/semantic parent). The upsert refreshes
  `prompt_id`, and the browser refetches the thread on `transcript_updated`
  (`frontend/packages/apps/web/src/data/applySessionEvent.ts` — pure refetch,
  so the re-emit cannot duplicate a row on screen). Clear the pending root on
  `TurnCompleted` too, so a prompt steered into a running turn (which keeps
  that turn's id, correctly, and gets no `turn/started` of its own) is never
  re-stamped by the *next* turn.

Rewrite `a_turn_id_becomes_the_prompt_group_and_seq_is_monotonic` to the live
ordering (`UserPromptAccepted`, then `TurnStarted`) and add unit tests in
`content.rs` (names are grep gates in `check_command`):

- `two_prompts_accepted_under_one_turn_id_get_distinct_uuids_and_keep_their_own_lane`
  — after `TurnStarted(t1)`, `begin_turn(main, None)` + prompt A, then
  `begin_turn(branch, Some(parent))` + prompt B with no `TurnStarted` in
  between: A and B have different uuids, A is on `main` with no semantic
  parent, B is on the branch thread with `parent`, both carry `prompt_id t1`;
- `a_prompt_accepted_before_its_turn_started_is_restamped_with_that_turn` —
  idle source (`TurnCompleted` seen), prompt accepted → `prompt_id None`; then
  `TurnStarted(t2)` returns exactly one message: the same uuid/seq/thread with
  `prompt_id t2`; a following `TurnStarted` returns nothing;
- `a_prompt_steered_into_a_running_turn_keeps_that_turn_and_is_not_restamped_by_the_next`
  — `TurnStarted(t1)`, prompt accepted (`prompt_id t1`), `TurnCompleted`,
  `TurnStarted(t2)` returns no message.

Also correct the doc comment on `start_turn` in `adapter/mod.rs` so it no
longer implies one prompt per turn, and stops describing the uuid as
turn-keyed if it does. `make gen-check` is unaffected (no wire change).

### Session-state coverage

The operation is "send a prompt to an open Codex session"; the turn states it
meets are: idle (existing `a_plain_turn_stays_on_main_with_no_semantic_parent`
plus the new restamp test), running turn with a plain send and with a branch
send (the new distinct-uuids test), and the turn ending after a steered send
(the new not-restamped test). A send to a still-spawning session is queued and
flushed on bind (`send_to_a_still_spawning_session_is_queued`) and is not
changed here.

### Pipeline notes

- Backend only; `make check` is the canonical gate. The appended gates assert
  the turn-keyed uuid is gone from the crate, the three new test names, and the
  `#[test]` count under `backend/crates/gateway/codex-agent/src/` (119 on
  `main`; at least three new tests → ≥ 122). All appended gates fail on `main`.
- The e2e-fake Codex scenarios emit `turn/started` after the `turn/start`
  response, like the real server, so they exercise Part B's restamp path and
  must stay green.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A Codex user prompt's uuid is minted from `seq` alone; the string
      `codex-turn-` no longer occurs under
      `backend/crates/gateway/codex-agent/src/` (negative grep gate).
- [x] Two prompts accepted under one running turn id — a plain one on `main`
      and a branch one routed by `begin_turn` — fold to distinct uuids, each on
      its own thread with its own semantic parent (unit test
      `two_prompts_accepted_under_one_turn_id_get_distinct_uuids_and_keep_their_own_lane`).
- [x] A prompt accepted while idle carries no `prompt_id`, and the next
      `TurnStarted` re-emits it once with that turn's id on the same
      uuid/seq/thread (unit test
      `a_prompt_accepted_before_its_turn_started_is_restamped_with_that_turn`).
- [x] A prompt steered into a running turn keeps that turn's `prompt_id` and is
      not re-stamped by the following turn (unit test
      `a_prompt_steered_into_a_running_turn_keeps_that_turn_and_is_not_restamped_by_the_next`).
- [x] The `#[test]` count under `backend/crates/gateway/codex-agent/src/` is
      at least 122.

### Manual / on-hardware (verified by a human before merge)

- [ ] In a live `make dev` Codex session: send a plain prompt, and while the
      turn is still running branch from a passage of the streaming reply with a
      second prompt. Both prompts remain visible — the first on the parent
      thread, the second at the top of the new branch thread — and the
      assistant's answer to the second follows it on the branch thread.
      (Non-blocking for merge under the CI-green autonomous policy; recorded
      for dogfooding.)

## Out of scope

- Queuing a Codex send composed mid-turn until the turn ends (Claude's
  single-outstanding rule). Codex steers such input natively and that is the
  behaviour dogfooding relies on; this task only makes the fold robust to it.
- Changing which columns the message upsert refreshes: keeping `thread_id` /
  `semantic_parent_uuid` first-ingest-authoritative is what protects Claude's
  branch attribution on re-ingest.
- A prompt whose `turn/start` fails after `UserPromptAccepted` was emitted is
  still persisted (pre-existing); its handling is a separate change.
- Repairing the two rows in the live dogfooding database (an operator action).
