---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'cd backend && cargo fmt --all -- --check && cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cd .. && BEFORE=$(find frontend/packages/gateway/wire-gen/src -type f -exec shasum -a 256 {} + | sort | shasum -a 256) && make gen && AFTER=$(find frontend/packages/gateway/wire-gen/src -type f -exec shasum -a 256 {} + | sort | shasum -a 256) && [ "$BEFORE" = "$AFTER" ] && cd frontend && pnpm -r build && pnpm -r typecheck && pnpm -r test && pnpm -r lint && cd .. && make e2e'
assignee: null
branch: task/0818-0619-fix-forked-skill-running-indicator
created_at: 2026-08-18T06:19:53Z
updated_at: 2026-08-18T11:15:00Z
---

# fix(subagent): light the running indicator for a forked background skill launched by a slash command

## Overview

A session started with a slash command that forks a background skill (e.g.
`/review-pr`, which Claude Code records as `/example:review-pr`) shows
**no running indicator at all** in the navigator for the entire time the skill
works — minutes, in practice — even though the session is very much busy. The
unread badge that fires when the work lands is unaffected, so the session row
sits completely inert and then suddenly goes unread. Reproduces every time.

Live evidence (dev DB + transcript, session
`01a0136c-98a0-7cb2-9f36-34f39b6c0cfb`, 2026-08-18, Claude Code v2.1.234).
The skill name below is a placeholder — the shape is what matters and is
reproduced exactly:

```
05:51:22  user   "/example:review-pr"          <- the local-command group's name line
05:51:22  system (subtype: local_command)
            <local-command-stdout>Running in the background as @example-review-pr</local-command-stdout>
            <forked-skill-launch>{"agentId":"a7046b32df40e1b3e",
                                  "skillName":"example:review-pr",
                                  "description":"/example:review-pr"}</forked-skill-launch>
          ... 183.8s of work in the forked agent, nothing in this transcript ...
05:54:28  user   <task-notification><task-id>a7046b32df40e1b3e</task-id><status>completed</status>…
05:54:33  assistant …
```

The send row correlated fine (`send.status = matched`), and `subagent_launch`
holds **no row** for the session.

Two mechanisms combine, and only the second one is a defect:

1. **The slash command is folded as a degenerate, already-finished turn** —
   working as designed. `resolve_line_thread`'s local-command branch
   (`backend/crates/domain/delta-attribution/src/attribute/thread_resolution.rs:32`)
   emits `Effect::SendMatched` + `Effect::LocalCommandTurnEnded` because a
   local command fires no `UserPromptSubmit` echo and no `Stop`; without it
   the dispatched send would wedge the queue forever
   (`backend/crates/domain/delta-usecase/src/interactor/sync/sync_transcript.rs:183`).
   So no turn is in flight and the `turn_started`-driven half of the indicator
   correctly stays dark.

2. **Nothing registers the forked skill as running** — the defect. The
   running-subagent indicator is lit exclusively from `Agent`/`Task`
   `tool_use` blocks on assistant lines
   (`backend/crates/domain/delta-attribution/src/attribute/content_blocks.rs:132`).
   A forked skill is launched by the CLI harness itself, not by the model, so
   the parent transcript carries **no `tool_use` block** — only the
   `<forked-skill-launch>` element above. Neither
   `Effect::SubagentLaunched` nor `Effect::SubagentIndicatorStarted` ever
   fires.

Both halves of the navigator's running signal are therefore dark
(`frontend/packages/apps/web/src/features/navigator/SessionNode.tsx:247`
ORs `runningThreads` with `runningSubagents`), which is exactly the reported
symptom. The unread path is untouched because it rides the `<task-notification>`
and the assistant lines that follow it.

### Fix

Treat `<forked-skill-launch>` as what it is: the launch of a **background**
subagent. The completion half already exists and needs no change — the
`<task-notification>` for a forked skill carries `<task-id>` (and no
`<tool-use-id>`), which
`thread_resolution.rs`'s `task_id` fallback already resolves into
`Effect::SubagentCompleted`.

1. **Detect and parse** the element in
   `backend/crates/domain/delta-attribution/src/claude_format/`, alongside
   the existing `task_notification_task_id` / `is_local_command_output`
   helpers: a new helper returns the payload's `agentId` (required),
   `skillName` and `description` for a line whose text carries a
   `<forked-skill-launch>` element with a JSON body. A line missing the
   element, or carrying an unparsable body, yields `None` (and the latter is
   logged, mirroring how a `<task-notification>` with neither correlation
   element is logged, so a future Claude Code format change surfaces in the
   logs instead of as a silently dark indicator).

   Note the shape: the element rides the `type: "system"` /
   `subtype: "local_command"` line, which delta ingests as `Role::Meta` via
   the parser's content check — that line carries **no `promptId`**, so it is
   *not* a member of the local-command `promptId` group. Detection must
   therefore be independent of `in_local_command_group`.

2. **Emit the launch effects** from the per-line fold
   (`attribute/attribute_lines.rs`), keyed by a synthetic
   `tool_use_id` derived from the payload's `agentId` (a forked skill has no
   real `tool_use` id; namespace it, e.g. `forked-skill:<agentId>`, so it can
   never collide with a genuine one):

   - `Effect::SubagentLaunched` with the launching thread **and the `task_id`
     already known** (`agentId`). The effect currently carries only
     `tool_use_id` + `thread_id` because a tool-launched background subagent
     learns its `agentId` later via `PostToolUse(Agent)`; extend it so a
     launch that already knows the id persists it in one step (the existing
     `pending_subagent_task_id` flush in `sync_transcript.rs` stays the path
     for the tool case). `state.launched_threads` must be seeded with the
     same `task_id` so a notification folded in the *same* window matches too.
   - `Effect::SubagentIndicatorStarted` with `background: true`,
     `subagent_type: skillName`, `description` from the payload.

   The launching thread is `state.carry_thread` — the same thread the group's
   lines and the later `<task-notification>` are attributed to, so the
   indicator, the messages and the unread suppression all agree.

3. Nothing else should need changing: `start_subagent` already de-duplicates
   by `tool_use_id`, the turn-end sweep deliberately keeps `background`
   entries (so `LocalCommandTurnEnded` cannot extinguish the fresh
   indicator), `close_session` / `on_session_end` already drain lingering
   background entries, and the frontend already folds `runningSubagents` into
   both the row spinner and the unread gate. Verify each of these rather than
   assuming.

Accepted behaviour, not a regression to fix here: a forked skill agent is
resumable, and its notification body says the same `task-id` may notify more
than once. The first notification consumes the entry; a later one matches no
launch and falls back to `carry_thread`, which is the existing
no-regression path for an unmatched notification.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A fold over a local-command group whose `local_command` line carries a
      `<forked-skill-launch>` payload emits `Effect::SubagentLaunched` (with
      the payload's `agentId` as `task_id`) and
      `Effect::SubagentIndicatorStarted` with `background: true`, the
      `skillName` as `subagent_type`, and the launching `carry_thread` —
      asserted by a unit test in `delta-attribution`.
- [x] A line with no `<forked-skill-launch>` element, and one whose element
      carries an unparsable/`agentId`-less body, emit neither effect —
      asserted by a unit test.
- [x] Ingesting the launch line broadcasts `SessionEvent::SubagentStarted`
      and leaves a running-subagent entry on the session runtime, and the
      entry **survives the `LocalCommandTurnEnded` turn end** emitted by the
      same group (the regression this task exists for) — asserted by an
      interactor test under
      `backend/crates/domain/delta-usecase/src/interactor/sync/tests/`.
- [x] A subsequent `<task-notification>` carrying only `<task-id>` (no
      `<tool-use-id>`) — the real forked-skill shape — finishes that entry:
      `SessionEvent::SubagentFinished` is broadcast, the runtime entry is
      gone, and the `subagent_launch` row is cleared — asserted by the same
      or an adjacent interactor test.
- [x] Re-ingesting the launch line (cursor rewind) neither duplicates the
      entry nor re-broadcasts `SubagentStarted` — asserted by a test.
- [x] The transcript-line test helpers
      (`backend/crates/domain/delta-usecase/src/interactor/testing/transcript_lines.rs`)
      gain a forked-skill-launch line builder used by the tests above, so the
      real Claude Code shape is pinned in one place.
- [x] The docs that describe where the running-subagent indicator comes from
      (`docs/guides/api/live-channels.md`, `docs/guides/api/sends.md` —
      whichever states the sources) name the forked-skill launch alongside
      the `Agent`/`Task` `tool_use` source; no doc still claims `tool_use` is
      the only source.

### Manual / on-hardware (verified by a human before merge)

- [ ] In the real dev app, starting a session with `/review-pr` lights the
      navigator row's running indicator within a second of launch, keeps it
      lit for the whole time the forked skill works (minutes), and clears it
      when the result lands — with the unread badge still appearing exactly
      as it does today.
- [ ] Closing the session while a forked skill is still running clears the
      indicator instead of leaving it stuck lit.

## Out of scope

- Non-forked slash commands (`/login`, `/clear`, an unknown command, a skill
  that runs inline in the main session). They keep today's degenerate
  zero-length-turn handling; this task only adds a signal for the forked
  case, and changes no existing turn-machine transition.
- Rebuilding the indicator for a forked skill across a session resume, or
  streaming the forked agent's own transcript into delta. Both match the
  existing behaviour for tool-launched background subagents.
