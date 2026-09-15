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
branch: task/0915-1526-fix-spend-a-hand-over-nobody-was-waiting-for
created_at: 2026-09-15T15:26:24Z
updated_at: 2026-09-15T16:27:20Z
---

# fix(workspace): spend a spawn's focus hand-over even when nobody is waiting for it

## Overview

A spawn's focus hand-over is meant to carry the user from the New session screen
into the session they just started. It is recorded on the tracked `SpawnItem`
(`focusHandedOver`) and consumed when focus moves.

When the `POST /api/sends` response lands while the user is **not** on the
new-session screen, the hand-over is deliberately declined — a spawn must not
steal focus from a session the user opened themselves. Today it is declined by
being left **unconsumed**, so it stays pending: the next time the user opens the
new-session screen, that spawn takes focus once. The user pressed **New session**
because they wanted the new-session screen, and instead they land on a starting
session they had already moved on from, and must press it again.

Being away at that moment means the user went and did something else — resumed a
conversation in another session, say. A hand-over nobody was waiting for should
be **spent unused**, not saved for later.

### Where the decision belongs

Decide it where the POST lands, in `useSubmitSend`
(`frontend/packages/apps/web/src/features/composer/useSubmitSend.ts`, the
`target.kind === 'new-session'` branch that calls `trackSpawn`), not in the
workspace effect.

The effect in
`frontend/packages/apps/web/src/features/workspace/WorkspaceScreen.tsx` reads
`isNewSessionFocus` (derived from `navStore`) alongside `spawns` (from
`liveStore`) and re-runs whenever either changes. Today `!isNewSessionFocus` is a
**gate**: it returns early, mutates nothing, and the next run can still hand over,
so a reading of "false" only ever means "not yet".

Making the effect consume the hand-over regardless would turn that gate into a
one-shot decision, and correctness would then rest on focus being on the
new-session screen in exactly the commit where `spawns` first contains the spawn
— a coincidence of two independent stores that nothing in the code enforces.
Anything that moves focus between the send and the response would silently spend
the hand-over instead of merely delaying it. `applySessionEvent` moving focus
itself when some other spawn fails is one such path; a reload restoring persisted
focus is another. The failure it produces is the symptom this whole area exists to
prevent: the user presses Send and is left where they were.

Reading `useNavStore.getState()` at the moment the POST resolves has no such
dependence on render timing, and puts the policy where the question is actually
asked: where was the user when the server answered.

Shape the change so `trackSpawn` records the spawn as already-handed-over when the
user was elsewhere. Whether that is an extra argument, a distinct action, or a
value computed by the caller is for the implementation to settle; `trackSpawn`'s
parameter type currently excludes `focusHandedOver` via `Omit`, so that type moves
with whatever shape is chosen. The workspace effect keeps its existing guard —
with the flag already set, the spawn simply never appears in `awaitingHandOver`.

### States

| when the POST response lands | expected |
| --- | --- |
| user is on the new-session screen | focus moves to the new session; hand-over consumed (unchanged) |
| user is on another session | no focus change, and the hand-over is spent — opening the new-session screen later leaves them there |
| user is on the new-session screen, an earlier spawn is still unconsumed | unchanged: the newest spawn is the one they are waiting on |
| the spawn is registered already `failed` (buffered failure) | unchanged: a failed spawn is a Retry / Dismiss card, never a hand-over candidate |

The comment at the target-selection block in `WorkspaceScreen.tsx` states that an
older unconsumed spawn "keeps its unconsumed hand-over and takes focus the next
time this screen is opened". That stops being true — correct it rather than
leaving it to mislead.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A test asserts that a spawn whose send is accepted while another session is
      focused does not take focus when the new-session screen is opened
      afterwards — the user stays on the new-session screen.
- [x] A test asserts the unchanged path still works: a send accepted while the
      new-session screen is focused moves focus to that session.
- [x] A test asserts a second session can still be started from the new-session
      screen while the first is `spawning`, and that focus moves to the second.
- [x] A test asserts a spawn registered already `failed` is unaffected: opening
      the new-session screen leaves focus there and the Retry / Dismiss card is
      still rendered.
- [x] The decision is taken from the focus as it stands when the POST resolves,
      not from a value read during a later render: a test drives a send whose
      response lands while focus is elsewhere and asserts the spawn is recorded
      as already handed over at that point, without the workspace effect having
      had to run.

### Manual / on-hardware (verified by a human before merge)

- [ ] In the running app: start a session, and while its POST is in flight click
      over to another session. When it has started, press **New session** — the
      new-session screen opens and stays open.
