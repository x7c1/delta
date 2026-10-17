---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check"
assignee: null
branch: task/1017-1644-keep-mid-turn-subagent-hand-backs-out-of-user-turns
created_at: 2026-10-17T07:44:03Z
updated_at: 2026-10-17T08:19:22Z
---

# fix(transcript): keep mid-turn subagent hand-backs out of user turns

## Overview

When a background subagent finishes, Claude Code hands its final report back
to the parent session as an `<agent-message from="…">` prompt. The transcript
records it in one of two shapes, depending on whether the parent was idle or
mid-turn when it arrived:

- **Idle:** a `type: "user"` line with `isMeta: true` and
  `origin: { kind: "peer", handback: true, … }`. The parser maps `isMeta` to
  `Role::Meta`, so it is not shown as a user turn. This works today.
- **Mid-turn:** a `type: "attachment"` line whose `attachment` is
  `{ type: "queued_command", commandMode: "prompt", isMeta: true,
  origin: { kind: "peer", handback: true, … }, prompt: "<agent-message …" }`,
  with `renderedRole: "system"` on the line. The parser's `queued_command` path
  (`backend/crates/gateway/delta-transcript/src/parse/mod.rs`, the
  `queued_prompt` / `is_queued_command` handling) maps every `queued_command`
  to `Role::User` without looking at the attachment's own `isMeta`, so the web
  UI renders the whole hand-back — the `<agent-message>` tag, the harness
  preamble, and the two-space-indented report — as a human user bubble.

The mid-turn shape is also an attribution hazard: as a `Role::User` line that
is not a task-notification, it counts as a human turn in
`backend/crates/domain/delta-attribution/src/attribute/attribute_lines.rs`, so
if a Delta send is outstanding when the hand-back is written, positional
correlation pops that send against the hand-back instead of the send's real
echo.

Fix it at the parser: a `queued_command` attachment whose attachment-level
`isMeta` is `true` is a harness injection and must classify as `Role::Meta`,
exactly like the idle shape, so both shapes reach attribution and the UI the
same way. Model `isMeta` on `RawAttachment`
(`backend/crates/gateway/delta-transcript/src/parse/raw_attachment.rs`).
Queued commands without attachment-level `isMeta` keep today's behaviour
(`Role::User` with `is_queued_command`), because mid-turn
`<task-notification>` deliveries use that shape (`commandMode:
"task-notification"`, no `isMeta`) and the task-notification card and
launch-thread resolution depend on it. Decide whether the meta line should
still carry `is_queued_command`, and justify the choice against how
`thread_resolution` treats a non-human line.

Survey of local transcripts (Claude Code 2.1.283–2.1.301): every
`queued_command` with `commandMode: "prompt"` had `isMeta: true` and a peer
hand-back origin; every one with `commandMode: "task-notification"` had no
`isMeta`. Real line shape (report text shortened):

```json
{"parentUuid":"a1","isSidechain":false,"attachment":{"type":"queued_command","prompt":"<agent-message from=\"a0000000000000001\">\n[Subagent hand-back] The text below is the final report of a subagent this session delegated to. …\n  result: success\n</agent-message>","commandMode":"prompt","origin":{"kind":"peer","from":"a0000000000000001","senderTaskId":"a0000000000000001","name":"general-purpose","body":"[Subagent hand-back] …","handback":true,"handbackReportText":"result: success\n…"},"timestamp":"2026-10-16T15:12:39.751Z","isMeta":true},"type":"attachment","uuid":"q1","timestamp":"2026-10-16T15:12:39.751Z","renderedRole":"system","sessionId":"…","version":"2.1.301"}
```

The comments and guides describe `queued_command` as a legacy-only shape
("older claude versions"), which is no longer true: current Claude Code
still writes it for harness injections delivered mid-turn (task
notifications and peer hand-backs); only human-typed queued prompts moved to
`queue-operation` + replay. `docs/guides/compatibility.md` ("Legacy-format
parsing") even says the branch may be removed at any time, which would break
mid-turn task notifications. Correct the vocabulary everywhere it states
this — at least `parse/mod.rs`, `raw_attachment.rs`, `raw_line.rs`,
`thread_resolution/mod.rs`, `attribute_lines.rs`,
`docs/guides/development/canary.md` and `docs/guides/compatibility.md` — so
the docs say which `queued_command` uses are legacy (human-typed prompts) and
which are current (harness injections). `git grep -n queued_command` lists the
sites.

Out of scope: rendering the hand-back report itself (unwrapping
`<agent-message>`, using `origin.handbackReportText`, Markdown display) — this
task only makes the mid-turn shape behave like the idle one.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A `delta-transcript` parser test feeds the mid-turn hand-back line shape
      above (attachment `queued_command`, `commandMode: "prompt"`,
      attachment `isMeta: true`, peer `origin`) and asserts `Role::Meta`.
- [x] A `delta-transcript` parser test feeds a mid-turn `queued_command` with
      `commandMode: "task-notification"` and no attachment `isMeta`, and
      asserts it still parses as `Role::User` with `is_queued_command`.
- [x] A `delta-attribution` test (fold test or corpus case) shows a mid-turn
      hand-back arriving while a send is outstanding does not consume that
      send and inherits `carry_thread`.
- [x] The existing legacy human-typed `queued_command` tests and corpus cases
      (`queued_command_branch`, `unmatched_queued_command`) still pass
      unchanged.

### Before merge (verified outside the check command)

- [x] In a `make dev` session, a background subagent whose report arrives
      while the main agent is still running a tool (recorded as a
      `queued_command` attachment with `commandMode: "prompt"`) shows as a
      meta line, not a user bubble.
