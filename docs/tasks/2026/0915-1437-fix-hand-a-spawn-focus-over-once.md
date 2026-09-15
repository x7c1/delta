---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, user-experience]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check'
assignee: null
branch: task/0915-1437-fix-hand-a-spawn-focus-over-once
created_at: 2026-09-15T14:37:24Z
updated_at: 2026-09-15T15:24:10Z
---

# fix(workspace): hand a spawn's focus over once, so New session stays usable

## Overview

Sending from the New session screen locks the user out of starting another
session until the first one binds. Pressing **New session** again appears to do
nothing: the button fires and focus does move to the new-session screen, but the
screen snaps straight back to the still-spawning session.

The cause is in `frontend/packages/apps/web/src/features/workspace/WorkspaceScreen.tsx:230-238`:

```ts
useEffect(() => {
  const spawning = spawns.filter((spawn) => spawn.status === 'spawning');
  if (spawning.length === 0 || !isNewSessionFocus) {
    return;
  }
  reconcileFocusedSession(spawning[spawning.length - 1].sessionId);
}, [spawns, isNewSessionFocus, reconcileFocusedSession]);
```

The effect is meant to be a one-shot hand-over: focus the session the server just
accepted, instead of parking the user on the new-session screen while the launch
comes up. But it is written as a *standing condition*. `isNewSessionFocus` stands
in for "the user has not navigated away since the send" — and that stand-in is
re-satisfied every time the user comes back to the new-session screen, so the
effect takes focus again.

The tracked spawn is released on `session_registered`
(`frontend/packages/apps/web/src/store/live/spawnsSlice.ts:364-368`), so the
lock-out lasts exactly until the session binds. When a launch stalls — an agent
waiting on an interactive first-run prompt, for instance — the spawn stays
`spawning` for the full `PENDING_SPAWN_DEADLINE`
(`backend/crates/domain/delta-usecase/src/interactor/session_actor/runtime/spawn.rs:23`,
30 seconds), and no new session can be started for that whole window.

Make the hand-over happen at most once per spawn: record on the tracked
`SpawnItem` that its focus has been handed over, and consume that when the focus
moves. Keep the guard the current condition was reaching for — a spawn must still
not steal focus from a session the user deliberately navigated to while the POST
was in flight.

The states the tracked spawn can be in when **New session** is pressed, and what
must happen in each:

| spawn state | expected |
| --- | --- |
| `spawning`, focus already handed over | the new-session screen stays focused |
| `spawning`, focus not yet handed over | focus moves to that session (today's hand-over, unchanged) |
| `failed` | the new-session screen stays focused; the Retry / Dismiss card is unaffected |
| no tracked spawn | unchanged |

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `WorkspaceScreen.test.tsx` asserts that once a spawn's focus has been handed
      over, returning to the new-session screen while that same spawn is still
      `spawning` leaves focus on the new-session screen.
- [x] `WorkspaceScreen.test.tsx` still asserts the original hand-over: focus moves
      to the spawned session when a send is accepted while the new-session screen
      is focused.
- [x] `WorkspaceScreen.test.tsx` asserts a spawn does not steal focus from a
      session the user navigated to while its POST was in flight.
- [x] `WorkspaceScreen.test.tsx` asserts that a `failed` spawn leaves the
      new-session screen focused when the user opens it.

### Manual / on-hardware (verified by a human before merge)

- [ ] In the running app, starting a session and then pressing **New session**
      while it is still starting opens the new-session screen and stays there, and
      a second session can be started from it.
