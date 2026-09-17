---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && ! grep -rq 'will not clear from this notification' backend/crates/domain/delta-attribution/src/"
assignee: null
branch: task/0917-1559-fix-match-a-keyless-task-notification-to-the-only-outstanding-launch
created_at: 2026-09-17T06:59:46Z
updated_at: 2026-09-17T07:41:48Z
---

# fix(attribution): match a keyless `<task-notification>` when exactly one launch is outstanding

## Overview

A background task's completion reaches the transcript as a harness-injected
`<task-notification>` user line. `resolve_thread` in
`backend/crates/domain/delta-attribution/src/attribute/thread_resolution.rs`
matches it to the launch that started the task by `<tool-use-id>`, falling
back to `<task-id>`; a match removes the entry from
`AttributionState::launched_threads`, emits `Effect::SubagentCompleted`
(which clears the persisted correlation and the browser's running indicator)
and attributes the continuation to the launching thread.

A notification whose body carries **neither** key matches nothing. Today
that case only logs a warning — "the running indicator will not clear from
this notification" — and the outstanding entry stays until the session is
closed. The user sees a subagent that reads as still running after it
finished.

When exactly **one** background launch is outstanding, there is no
ambiguity about which task a keyless notification reports: it is that one.

### Change

- In the keyless branch (both `task_notification_tool_use_id` and
  `task_notification_task_id` are `None`), if `state.launched_threads` holds
  exactly one entry, resolve to it and take the same path a keyed match
  takes: remove the entry, push `Effect::SubagentCompleted`, move
  `carry_thread` to the launching thread.
- With zero or two-or-more outstanding launches the behaviour is unchanged:
  no entry is consumed and the line inherits `carry_thread`. Guessing among
  several would clear the wrong indicator and misattribute the continuation,
  which is worse than a stale indicator.
- A notification that carries a key which matches nothing is **not** keyless
  and is not touched by this change: a key that names an unknown launch is
  evidence the notification belongs to some other launch (an earlier window),
  so it must not consume the one that happens to be outstanding.
- Logging: the keyless case stays visible, because it signals an upstream
  format change. When the single-entry rule matches, log at `warn` that a
  keyless notification was matched to the only outstanding launch (with the
  session id and the matched `tool_use_id`). Otherwise keep a warning that
  says it could not be matched, and include the outstanding count so "none"
  and "ambiguous" can be told apart in a captured log.
- Update the comment block above the branch so it describes the three
  outcomes (keyed match, keyless single match, no match), and any doc under
  `docs/` that describes notification matching.

### Tests

Unit tests beside the existing `thread_resolution` / attribution tests, one
per state of the outstanding set when a keyless notification arrives:

- exactly one outstanding launch → `SubagentCompleted` for it, entry
  removed, the line is attributed to the launching thread, `carry_thread`
  moves;
- no outstanding launch → no effect, inherits `carry_thread`;
- two outstanding launches → no effect, neither entry removed, inherits
  `carry_thread`;
- a notification with a `<task-id>` that matches nothing while one launch is
  outstanding → no effect, the entry stays (the guard above).

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A keyless `<task-notification>` with exactly one outstanding launch
      completes that launch (effect, entry removal, thread attribution),
      covered by a unit test.
- [x] Keyless with zero and with two outstanding launches consumes nothing,
      each covered by a unit test.
- [x] A notification carrying a key that matches no launch consumes nothing
      even when one launch is outstanding, covered by a unit test.
- [x] The old warning text "the running indicator will not clear from this
      notification" is gone as an unconditional statement (`check_command`
      greps that the exact phrase no longer appears in the crate).
