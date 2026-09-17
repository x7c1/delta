---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, user-experience]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && ! grep -rq 'spawningIds' frontend/packages/apps/web/src/features/composer/"
assignee: null
branch: task/0917-1429-fix-keep-a-starting-session-first-send-off-the-new-session-screen
created_at: 2026-09-17T05:29:37Z
updated_at: 2026-09-17T06:11:01Z
---

# fix(composer): keep a starting session's first send off the new-session screen

## Overview

The new-session screen's pending strip lists, besides the submits still
awaiting their POST (`sending` rows), the first send of **every** session
that is still `spawning`. That is the new-session branch of
`frontend/packages/apps/web/src/features/composer/usePendingSends.ts`: it
collects the tracked local send of each `spawning` spawn and emits it as a
`local` row.

A `local` row carries no label, no spinner and no Cancel
(`PendingQueue.tsx`). So a user who starts a session, goes elsewhere while it
launches, and then presses New session finds the text of the session they
started earlier sitting as a bare line on the screen where they are about to
write the next one. It reads as if the new send had already gone out, and it
cannot be dismissed until the launch registers.

The row once had a job: between a send being accepted and the workspace
moving focus to the new session, the strip was the only place its text could
appear. That is no longer true. Focus is handed over the moment the send is
accepted, and a starting session — and a failed one — now has its own row in
the navigator and its own screen, where its first prompt is shown. The
new-session strip repeating that text says the same thing in a second place,
on a screen that has nothing to do with that session. (A label such as
`starting — <branch>` on the row was considered and rejected for the same
reason: it would decorate a duplicate rather than remove it.)

### Change

- In the new-session branch of `usePendingSends`, stop emitting `local` rows
  for spawning sessions. The branch returns only the `sending` entries whose
  target is `new-session`. Drop whatever the hook then no longer needs (the
  `spawns` subscription and its memo dependency, if nothing else in the hook
  reads it) and rewrite the branch's comment to describe what is left and
  why a starting session's send is absent — next to the existing sentence
  about a failed launch, which makes the same argument.
- The `thread` branch is unchanged: on a starting session's **own** screen
  the first send still shows in the strip. The existing tests for that are
  the guard.
- Sweep comments elsewhere that describe the removed row (for example the
  `PendingQueue.tsx` comments about a `local` chip on the new-session
  screen, and any doc under `docs/` that describes the strip), so none
  claims the new-session screen lists a starting session's send.

### Tests

- `usePendingSends` (or the nearest existing test of the new-session
  surface): with one `spawning` spawn that has a tracked local send and no
  in-flight submit, the new-session surface yields no entries; with an
  in-flight new-session submit it yields exactly that `sending` entry.
- Update or remove existing cases that asserted the removed row.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A test shows the new-session surface yields no entry for a `spawning`
      session's tracked first send, and still yields the `sending` entry of an
      in-flight new-session submit.
- [x] A test shows the same send still appears on the starting session's own
      thread surface.
- [x] The new-session branch no longer derives a spawning-id set
      (`check_command` greps that `spawningIds` is gone from
      `features/composer/`).

### Manual / on-hardware (verified by a human before merge)

- [ ] In the mock frontend (`make mock`): start a session, press New session
      while it is still starting, and confirm the strip above the composer is
      empty; the starting session's own screen still shows its first prompt.
