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
branch: task/0915-1438-feat-attach-the-terminal-while-a-session-is-starting
created_at: 2026-09-15T14:38:00Z
updated_at: 2026-09-16T03:27:58Z
---

# feat(terminal): attach the embedded terminal while a session is still starting

## Overview

Between the moment a fresh spawn is accepted and the moment it binds, the tmux
pane exists and may be waiting for input — but the browser cannot reach it. The
pane is the product's own escape hatch for answering an agent's interactive
prompts, and during this window it is sealed off.

This was found in dogfooding. Starting a session in a repository Claude Code had
never been trusted in left the pane sitting on Claude Code's workspace-trust
dialog (`Quick safety check: Is this a project you created or one you trust?`).
Nobody could answer it, so no `SessionStart` hook ever arrived, and after 30
seconds `reap_stale_spawns` killed the pane and reported `SpawnFailed` with no
reason — twice in a row, because retrying changed nothing.

### What closes the path today

- The right-hand column itself is **not** gated on session state:
  `showTerminalPane = terminalOpen && focusedHasTerminal`
  (`frontend/packages/apps/web/src/features/workspace/WorkspaceScreen.tsx:461`),
  so it is already on screen during `spawning`.
- What is closed is the attach inside it. `attachable` receives
  `focusedItem?.open` (same file, lines 408 and 483), and
  `canAttach` (`frontend/packages/apps/web/src/features/terminal/TerminalPane.tsx:80`)
  is false. `open` means "bound", so it is false for a spawning session.
- The server refuses in any case: `pane_for_session`
  (`backend/crates/domain/delta-usecase/src/interactor/routing.rs:292`) reads
  `ctx.state.handle()`, the bound handle, so `bridge()` closes the socket
  (`backend/crates/apps/delta-server/src/pty.rs:84-90`).
- Because `attachable` folds "has a bound pane" and "may be attached to" into one
  boolean, a spawning session falls into the closed branch of `unavailableNote`
  (`TerminalPane.tsx:178-184`) and is described as
  `This session is closed. Resume it to attach its terminal.` — untrue, and it
  tells the user to do something they cannot do.

### What already exists

The actor already draws the distinction this needs. `QueryIsOpen` reports bound;
`QueryIsLive` reports `has_live_pane`, which by its own definition covers "a
spawn whose pane is up awaiting its first bind" and "one accepted with its launch
preparation still running"
(`backend/crates/domain/delta-usecase/src/interactor/session_actor/actor.rs:327-332`,
`routing.rs:307-312`). The three-way state exists in the runtime; it is simply
not carried to the PTY route or the UI. Focusing the session screen the instant a
send is accepted also already ships.

### What is missing

`SessionEvent` has no variant announcing that a pane came up, so the browser
hears nothing between accept and bind. Emit one when the launch reaches
`finish_launch` (`backend/crates/domain/delta-usecase/src/interactor/lifecycle/`),
carrying the session id and pane token, and let the browser attach on it. Client
polling and holding the `/pty` socket open server-side were both considered and
rejected: the first sprays failing connects, the second blurs the existing guard
that refuses attaches to sessions that are not open. The browser is event-driven
around `applySessionEvent`, and the UI needs the signal anyway to tell
"preparing" apart from "starting, terminal available".

### Scope

1. Emit the pane-ready event at `finish_launch`.
2. Let `pane_for_session` resolve the pane of a launched-but-unbound spawn.
3. Widen `attachable` from two states to three — no pane yet / pane up but
   unbound / bound — and attach in the middle state.
4. Correct `unavailableNote` so a starting session is described as starting.
5. Stop the reaper from killing a pane while a PTY bridge is attached to it
   (`reap_stale_spawns`). Redesigning the deadline itself is out of scope; only
   the "do not kill what someone is using" rule belongs here.
6. Skip the pre-attach `clear_session_input` (`pty.rs:66`) for an unbound attach,
   or establish that it is harmless there. It exists to wipe residual input in
   Claude's composer; sending those keys to a pane showing a selection dialog
   would answer the dialog by accident.

7. When the watchdog gives up on a launch, capture what the pane was showing and
   persist it as the failure's reason, so the failed session's screen can say
   what happened instead of that nothing was heard. `reap_stale_spawns` passes
   `reason: None` today, and the screen falls back to wording that says Delta
   never learned why — which is the screen a stalled launch reaches most often.
   Delta does know one thing even then: the launch did not bind before its
   deadline. Say that, and add the pane's last lines when they are available.
   `TmuxDriver` has no capture method yet; the real-agent canary already shells
   out to `tmux capture-pane -p`, so adding one port method, its gateway
   implementation and a fake is the whole of it.

   This item exists only because a failed launch now keeps its row and its own
   screen. Attaching (items 1-6) serves the user who is watching; this serves the
   one who was not, and both are the same underlying problem — the pane knows why
   and nobody can reach it.

A session cannot be attached to at the instant of send: the pane is created at the
end of launch preparation (`prepare_pane_launch`'s `create_session`). Observed
preparation times range from `prepared_in_ms=0` (no worktree) to
`prepared_in_ms=3992` (worktree checkout). That window has nothing to attach to
and must say so plainly.

The states a session can be in when the terminal pane renders, and what each must
show:

| session state | expected |
| --- | --- |
| accepted, launch preparation still running | no attach; a message saying the session is being prepared |
| pane launched, not yet bound | attached and interactive |
| bound / open | attached and interactive (unchanged) |
| closed | no attach; the existing "closed, resume it" message (unchanged) |
| spawn failed | no attach |

Adding a `SessionEvent` variant regenerates wire types. The orchestrator runs the
canonical gate over a temporary WIP commit for this reason: `gen-check` compares
the generated tree against the committed one, so an uncommitted regeneration reads
as stale. Do not try to work around it from inside the work phase.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] An e2e-fake spec, with bind delayed so the session stays `spawning`,
      asserts the terminal attaches and shows the pane's live output while the
      session is still starting.
- [x] A spawning session's terminal is never described as closed: the spec
      asserts the `This session is closed.` text is absent while spawning, and
      that the preparing-state message appears before the pane exists.
- [x] A backend test asserts the PTY pane lookup resolves a launched-but-unbound
      spawn's pane, and still resolves nothing before the pane is created, after
      the spawn fails, and for a closed session.
- [x] A backend test asserts the stale-spawn reaper leaves a pane alone while a
      PTY bridge is attached to it, and still reaps one with no client attached.
- [x] A test asserts the input-clearing keys are not sent when attaching to a
      pane that has not bound yet.
- [x] The existing bound-session attach behaviour is unchanged: the e2e-fake
      terminal specs that cover an open session still pass untouched.
- [x] A backend test asserts that a spawn reaped at its deadline records a reason
      naming the deadline, rather than leaving it empty, and that the pane's
      captured output is included when the pane was still alive to read.
- [x] An e2e-fake spec asserts the failed session's screen shows that reason
      instead of the "nothing was heard" fallback.

### Manual / on-hardware (verified by a human before merge)

- [ ] In the running app, starting a session in a repository Claude Code has not
      been trusted in shows the workspace-trust dialog in the embedded terminal;
      answering it lets the launch continue, the session binds, and the first
      prompt is delivered.
