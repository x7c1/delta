---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: make check
assignee: null
branch: task/1011-2218-fix-keep-the-echo-deadline-off-a-fresh-spawns-first-prompt-until-it-binds
created_at: 2026-10-11T13:18:18Z
updated_at: 2026-10-11T13:37:25Z
---

# fix: keep the echo deadline off a fresh spawn's first prompt until it binds

## Overview

A fresh Claude session whose launch takes longer than the echo deadline
(`ECHO_DEADLINE`, 60s) before `claude` is ready receives its first prompt
**twice**, and runs it twice.

Observed on a dev build: a new session was started from the composer in a
directory that is not a git repository, so Delta did not pre-accept Claude
Code's folder-trust dialog and `claude` stopped on it. The server log and the
session's transcript show:

1. `spawn_fresh` (`interactor/lifecycle/spawn_fresh.rs`) enqueues the first
   prompt as a send, applies `TurnInput::dispatch` so the turn machine is in
   `AwaitingEcho`, and passes the prompt to `claude` as the trailing positional
   argument, which `claude` auto-submits once it starts.
2. `claude` sat on the trust dialog. After 60s the echo-deadline sweep
   (`interactor/echo_deadline.rs`, `sweep_echo_deadline`) fired: "dispatched
   send produced no echo …", then "outstanding send never echoed; returning it
   to `queued`". The sweep's flush typed nothing, because the pane was not
   bound yet.
3. When the dialog was answered, `claude` started and began running the
   positional prompt. `SessionStart(startup)` bound the pending spawn, and the
   flush posted on bind (`dispatch_queued_send`,
   `interactor/enqueue/dispatch_queued.rs`) typed the requeued row into the
   pane with `send_line`.
4. Claude Code queued the typed copy while the first turn ran and submitted it
   after that turn ended: two identical user messages, 0.1s apart in Claude
   Code's queue, two `UserPromptSubmit` hooks.

The cause is that `SessionRuntime::expired_echo_deadline`
(`interactor/session_actor/runtime/turn.rs`) exempts only a resuming session
(`is_resuming`), whose held first prompt has not been typed yet. A fresh spawn's
first prompt is in the same position: it has not been typed into anything, it
rides the command line and lands when `claude` is ready. While the spawn is
launching or pending (`SessionRuntime::is_launching_or_pending`), the
watchdog for "the launch never became ready" is the pending-spawn deadline
(`PENDING_SPAWN_DEADLINE`, reaped by `take_stale_pending`), not the echo
deadline. The echo deadline's recovery (an `Escape` into the pane, then a
re-type) is wrong for this window: the re-type duplicates a prompt that is still
on its way, and the `Escape` goes to whatever dialog `claude` is showing, which
in the trust dialog's case may be its cancel key.

This affects any launch that takes over 60s to become ready: a trust dialog in
a user-selected non-git directory, a login or update prompt, slow MCP startup.

### What to change

1. **No echo deadline while a fresh spawn is unbound.** `expired_echo_deadline`
   returns `None` while the session is launching or pending, as it does while
   resuming. Update the doc comments that describe what the sweep exempts
   (`expired_echo_deadline`, `sweep_echo_deadline`, and the module doc of
   `echo_deadline.rs` where it lists the exemptions).
2. **The deadline counts from the bind, not from the launch.** When the spawn
   binds (`bind_pending_spawn`, reached from `SessionStart(startup)` or the first
   `UserPromptSubmit`), a first send still in `AwaitingEcho` gets a fresh
   `awaiting_echo_since`, so the sweep does not fire on the first tick after a
   long launch, in the gap between `SessionStart` and the `UserPromptSubmit` of
   the auto-submitted prompt. If the positional prompt then really never
   echoes, the normal deadline applies from the bind.
3. **The adapter-backed spawn path.** Check whether
   `lifecycle/adapter_session/spawn_adapter_session.rs` puts its first send in
   `AwaitingEcho` before the agent is connected in a way the same sweep can
   requeue. If it does, apply the same rule there; if it does not, say why in
   the PR.

Keep the echo deadline's behaviour unchanged for a bound session: a typed send
whose keystrokes are swallowed by a modal is still retried once and then
parked.

### Out of scope

- Pre-accepting the trust dialog for user-selected non-git directories.
- Changing `ECHO_DEADLINE` or `PENDING_SPAWN_DEADLINE`.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A use-case test spawns a fresh session with a first prompt, advances the
      clock past the echo deadline while the spawn is still unbound, runs the
      echo-deadline sweep, and observes that the send stays `dispatched`, no
      `Escape` and no keystrokes reach the pane, and the turn stays
      `AwaitingEcho`.
- [x] A test binds that spawn after the long wait and runs the sweep right
      after the bind: nothing is requeued or typed, and the
      `UserPromptSubmit` of the positional prompt settles the send as its echo,
      so the pane receives no `send_line` for it at any point.
- [x] A test shows that a bound session whose typed send never echoes is still
      requeued and re-typed once after the deadline (existing behaviour kept).
- [x] `make check` passes.

### Before merge (verified outside the check command)

- [x] On this Linux machine with `make dev` (dev data directory, not the
      installed Delta's), start a new session from the composer with a first
      prompt in an empty non-git directory that Claude Code has not trusted,
      leave the trust dialog up for more than 60 seconds, then accept it. The
      session's transcript contains the prompt once, and the server log shows
      no "produced no echo" warning for it.
