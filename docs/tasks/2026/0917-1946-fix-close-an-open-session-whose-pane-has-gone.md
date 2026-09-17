---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure, user-experience]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check"
assignee: null
branch: task/0917-1946-fix-close-an-open-session-whose-pane-has-gone
created_at: 2026-09-17T10:46:21Z
updated_at: 2026-09-17T12:02:47Z
---

# fix(session): close an open session whose pane has gone

## Overview

An open Claude session is "open" because its actor holds a bound pane handle
(`SessionRuntime::open`). Nothing ever checks that the pane behind the
handle still exists. If `claude` dies without delivering a `SessionEnd` hook
— killed, crashed, the tmux session removed from outside — or exits normally
(the `SessionEnd` path in `interactor/hooks/on_session_end.rs` deliberately
leaves the binding alone), the session keeps reading as open:

- the list shows it open, and the embedded terminal tries to attach to a
  pane that is not there;
- a send is typed into the dead pane, `send_line` fails, the send is
  cancelled (`enqueue/enqueue_into_open.rs`) and the user gets an error —
  every time, until they think of pressing Close, after which the same send
  resumes the session and works;
- a background subagent that was running stays on screen as running until
  that Close, because only the process-gone sweep clears it and nothing
  calls it (no hook arrived).

A closed session already behaves correctly — a send resumes it — so the fix
is to notice the pane is gone and close the session.

### Change

- On the existing background tick that already probes panes for launches
  (the reap sweep: `Interactor::reap_stale_spawns` →
  `SessionInput::ReapTick` → `lifecycle/reap_stale_spawns.rs`, which uses
  `TmuxDriver::has_session`), also check each **open, pane-backed** session:
  if `has_session(token)` says the pane is gone, close the session. If the
  check reads better as its own tick input / module than as part of the
  launch reaper, make it one — but do not add a second timer in the server;
  reuse the cadence that exists.
- "Close" means the teardown `SessionContext::close_session`
  (`lifecycle/close_session.rs`) gives a bound session, minus killing a pane
  that is already gone: one last `sync_transcript` so a straggler final line
  is ingested, drop the binding (`remove_open`), feed `TurnInput::Close`,
  run `sweep_running_subagents_on_process_gone`, and report
  `SessionEvent::SessionClosed` plus the sweep's events so the browser
  refetches. Share that code with `close_session` rather than copying it.
- Scope and guards:
  - Only a session with a bound pane handle. An adapter-backed (Codex)
    session has no pane and its adapter already reports process exit
    (`AgentEvent::SessionEnded`); leave it alone.
  - Not while the session is `resuming` (bound but not ready): that window
    belongs to the resume reaper and its deadline, which reports a failed
    resume rather than a close.
  - A probe **error** (tmux itself failing) is not "gone": log it and leave
    the session open. Only a definite "no such session" closes.
  - An age limit on open sessions was considered and rejected: a long-lived
    healthy session must never be closed by a timer.
- Log the closure at `warn` with the session id and token — it means a
  process went away without telling Delta.
- Update `docs/guides/` where session lifecycle or the background ticks are
  described, and the comment in `on_session_end.rs` that says the normal-end
  path leaves close semantics alone, to say what now closes the session
  afterwards.

### Tests

Usecase tests with the fake tmux driver (`interactor/testing/`), one per
state the session can be in when the tick fires:

- open, pane present → stays open, no events;
- open, pane gone → session closed (`is_session_open` false),
  `SessionClosed` emitted, a running background subagent is swept
  (`SubagentFinished`), an in-flight turn is closed, and a following send
  takes the resume path instead of failing;
- open, probe errors → stays open;
- resuming (not ready), pane gone → untouched by this check (the resume
  reaper's own test still describes what happens);
- closed / never opened → nothing happens;
- adapter-backed open session → untouched.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] An open pane-backed session whose pane no longer exists is closed on
      the background tick, emitting `SessionClosed` and sweeping running
      background subagents (usecase test).
- [x] After that closure a send resumes the session rather than failing
      (usecase test).
- [x] A present pane, a probe error, a resuming session, a closed session
      and an adapter-backed session are each left untouched (one usecase
      test each).

### Manual / on-hardware (verified by a human before merge)

- [ ] Reading the diff confirms the bound-session teardown is one shared
      routine used by both the explicit close and the pane-gone close (the
      pane kill being the only difference).
- [ ] Against a real server: open a session, kill its tmux session from
      outside (`tmux -L <the server's socket> kill-session -t <token>`),
      and confirm the session turns closed in the navigator within a tick and
      the next send resumes it.
