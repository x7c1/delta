---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, user-experience]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check"
assignee: null
branch: task/0929-1055-feat-open-the-terminal-per-session-and-detach-an-unfocused-starting-pane
created_at: 2026-09-29T10:55:38Z
updated_at: 2026-09-30T01:52:00Z
---

# feat(terminal): remember the terminal per session, open it for a new one, and detach an unfocused starting pane

## Overview

A session's pane comes up before anything binds it, and that window is when
the embedded terminal matters most: Claude Code can stop on the
workspace-trust dialog (or auth), which fires no hook, so nothing but a human
at the pane can get it moving. delta#403 made that pane attachable while the
session is `starting`, but the terminal column is closed by default
(`navStore.ts` `terminalOpen: false`, persisted as one global flag), so the
person who hits the dialog — typically someone launching in a repository for
the first time — sees a session that says "starting" and nothing that says
the pane is asking them something. After the bind deadline
(`PENDING_SPAWN_DEADLINE`, 30 s) the reaper kills the pane and the failed
screen quotes what it was showing; by then Retry only hits the same dialog.

Opening the terminal by default was tried and collided with the reaper:
`SessionRuntime::take_stale_pending`
(`backend/crates/domain/delta-usecase/src/interactor/session_actor/runtime/spawn.rs:327`)
never reaps a pane with a PTY bridge attached, on the premise that an attach
is a person watching. `TerminalPane.tsx` keeps every session's bridge alive
while the column is open (so a refocus does not re-run the pre-attach input
wipe and put a stray blank line into Claude's input), so with the column open
by default every starting session was "watched" whether anyone looked at it
or not, and a hung launch never reached the failed screen (e2e-fake
`spawn-failure.spec.ts` went red). The decision is to keep the default-open
behaviour and instead stop holding a bridge on a starting pane nobody is
looking at.

### Change

1. **Terminal open/closed is per session.** Replace the single persisted
   `terminalOpen` boolean in `frontend/packages/apps/web/src/store/navStore.ts`
   with a per-session record keyed by session id, persisted like the rest of
   the layout state (`partialize`). `setTerminalOpen` / `toggleTerminal`
   act on the focused session. A session with no saved value counts as
   **open** on the large-screen layout (the persistent pane), so a new
   session starts with its pane visible; on the small-screen layout the
   terminal is a slide-in overlay that covers the conversation, so there the
   unset value counts as **closed** as today. Read the layout the same way
   `WorkspaceScreen.tsx` already tells the two apart. Prune saved entries
   for sessions that are no longer listed rather than letting the record grow
   forever. Drop or ignore the old boolean key on rehydrate; do not migrate
   it. The New session screen has no session to key on — keep what it does
   today. `commsOpen` (the Codex comms-log pane) is out of scope and stays a
   single flag.
2. **An unfocused starting pane holds no bridge.** In
   `frontend/packages/apps/web/src/features/terminal/TerminalPane.tsx` the
   effect that shows the focused pane keeps the other entries attached but
   hidden. Keep that for entries whose session is bound (`open`): the
   stray-blank-line reason still holds. But an entry built against a
   `starting` pane is torn down (`disposeEntry`) when its session loses
   focus, so the server's `note_pty_detached` fires and the bind deadline
   runs again from that detach. Focusing it again rebuilds the entry
   (re-attaching a starting pane repeats the pre-attach wipe — check that it
   is harmless on the trust dialog, as delta#403 flagged, and note
   what you found in the PR). The component currently only receives the
   focused session's `paneState`; record on each `PaneEntry` which state it
   was built against, or pass the extra state in, whichever reads better.
   Update the comments in that effect, which explain why entries are kept,
   to say why a starting one is not.
3. **Tests.** `frontend/packages/apps/web/e2e-fake/spawn-failure.spec.ts`:
   - the first case (the hang fails while the user is on another session)
     must pass unchanged in behaviour — the doomed session loses focus, its
     bridge is dropped, the deadline runs, the row turns failed. Adjust
     timeouts only if the restarted deadline needs it;
   - rewrite the second case (the failure lands on the screen you are on)
     to close the terminal column for that session first, since with the
     column open the pane is watched and the launch is deliberately not
     reaped;
   - add a case for exactly that: with the terminal open on the starting
     session, the launch does **not** turn failed after the deadline, the
     pane stays attachable, and closing the session from the card's kebab
     `Close` cancels the launch (`spawn_failed.cancelled`, delta#379).
   Unit tests for the store cover: unset defaults to open on the large
   layout and closed on the small one; toggling one session leaves another's
   value alone; pruning on a refetched list; rehydrating a stored record.
   Cover the TerminalPane teardown rule in its existing unit tests if it has
   any that reach the effect; otherwise the e2e cases above are the proof.
4. **Docs.** If `docs/guides/` describes the terminal pane's open state or
   the "attached means watched" premise of the reaper, update it to say the
   frontend now guarantees that premise for starting panes by holding a
   bridge only on the focused one.

No backend change is expected: `take_stale_pending`, `note_pty_detached`
and the deadline restart already do the right thing once the frontend stops
holding the bridge.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] The terminal open state is stored per session and persisted; a
      session without a saved value is open on the large layout and closed
      on the small one (store unit tests).
- [x] Toggling the terminal for one session does not change another's, and
      entries for sessions no longer listed are pruned (store unit tests).
- [x] e2e-fake `spawn-failure`: a hung launch the user has moved away from
      turns failed at the deadline; a hung launch whose terminal the user
      closed turns failed on its own screen; a hung launch the user is
      watching with the terminal open does not turn failed and is cancelled
      by Close.
- [x] `make check` is green: frontend build, typecheck, lint, unit tests, and
      the e2e-fake lane.

### Manual / on-hardware (verified by a human before merge)

- [ ] Against a real server on a large screen: start a session in a
      directory Claude Code has not trusted yet; the terminal column opens
      with the new session and shows the trust dialog without any click;
      answering it in the pane lets the launch bind, and the terminal stays
      attached through the bind (no blank line lands in Claude's input).
- [ ] Focus another session while the first is still starting, come back,
      and confirm the pane re-attaches and the dialog is still answerable.
- [ ] Close the terminal for one session, focus another, and confirm the
      second still opens by default while the first stays closed after a
      reload.

## Out of scope

- The Codex comms-log pane (`commsOpen`) and its default.
- Changing the reaper, the bind deadline, or `take_stale_pending`.
- A visual mark on the Terminal button for "the pane is asking something";
  delta cannot tell that from "the pane is up" before the bind, so the
  default-open pane is the signal.
