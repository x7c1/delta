---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure, error-type-design, user-experience]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check"
assignee: null
branch: task/1002-0300-feat-re-adopt-sessions-that-survived-a-restart
created_at: 2026-10-01T16:59:28Z
updated_at: 2026-10-01T18:20:01Z
---

# feat(session): re-adopt sessions that survived a Delta restart

## Overview

Claude Code sessions run in tmux panes on Delta's own tmux server and keep running
when Delta stops — the desktop app is quit or upgraded, it crashes, or a dev server is
restarted without `make down`. That survival is intended: the user can attach with
`tmux -L <socket> attach` and keep working. But a restarted Delta does not pick these
sessions up again:

- Whether a session is open, and which pane it lives in, is process-runtime state
  only (there is no column for it; see the note in
  `backend/crates/gateway/delta-sqlite/src/migrations/session.rs`). Session actors are
  created lazily and the periodic ticks (transcript tail, liveness reap, echo
  watchdog) only reach existing actors
  (`interactor/session_actor/registry.rs`, `interactor/routing/ticks.rs`).
- The `TmuxDriver` port (`backend/crates/domain/delta-usecase/src/ports/tmux_driver.rs`)
  can check, create, send to and kill a session, but nothing at boot asks which
  sessions still exist.
- So every surviving session reads as closed. The terminal column cannot attach to
  its pane (`/pty` resolves panes through the registry). Sending to it goes through
  `ensure_open` → `open_session` (`interactor/lifecycle/open_session.rs`), which
  starts `claude --resume <id>` in a **new** pane while the old process is still
  running: two Claude Code processes then drive the same conversation and can edit
  the same files concurrently.
- `docs/guides/install/README.md` says open sessions "are picked up again on the next
  launch", which is not true today.

The hook port and secret are now stable across restarts (a previous change persists
them), so a surviving session's hooks reach the restarted server. What is missing is
the binding between the session row and its live pane.

What to build:

- **Remember the pane.** When a Claude session is bound to its tmux session/pane
  (fresh spawn and resume both go through `OpenHandle`), persist enough to find it
  again — the tmux session name (`delta-<n>`) and pane id — on the session row (a
  migration), and clear it when the session closes.
- **Re-adopt at boot.** On server start, for each session that has a remembered tmux
  session: if it still exists on Delta's socket, bind it as open (create its actor,
  bind the `OpenHandle`) **without** resuming, catch the transcript up from the stored
  cursor so output produced while Delta was down appears, and let the normal ticks
  (tail, liveness, echo watchdog) take over. If it no longer exists, clear the
  remembered pane and leave the session closed. Do this before the server starts
  accepting requests, or make sure a send that races it cannot spawn a second
  process.
- **Never double-launch.** As a backstop independent of boot timing, `open_session`
  must not start `claude --resume` for a session whose remembered tmux session is
  still alive; it adopts that pane instead.
- **Unreachable hooks.** When the server reports that the hook endpoint changed since
  the previous run (`delta_bootstrap::Config::hook_endpoint_changed`: the persisted
  port was taken and the app fell back to another one, or the secret was minted
  afresh), re-adopted Claude sessions cannot deliver hooks. The flag says surviving
  sessions *would* be stranded, not that any exist (it is also true on a first run),
  so act on it only for sessions actually re-adopted. Mark them so the UI shows a
  one-line notice on the session: Delta lost contact with this session; use the
  terminal, or Close it and send again to resume. Close kills the old pane, and the
  next send resumes with fresh settings, which restores everything.
- **One desktop app at a time.** Launching a second copy of the desktop app today
  starts a second server on the same database and tmux socket; with the persisted
  port it also fails to bind the recorded port, falls back, overwrites the record and
  reports the endpoint as changed, so the next launch strands the first copy's
  sessions. Make the desktop app single-instance (for example Tauri's single-instance
  plugin): a second launch focuses the running window and exits. The CLI server is
  unaffected.
- **Codex.** Codex sessions run as children of the Delta process (`codex app-server`,
  `kill_on_drop`) and end with it; there is nothing to re-adopt, and the next send
  resumes the thread (`thread/resume`). Leave that path as is and say so in the
  install guide.
- **Docs.** Make `docs/guides/install/README.md` describe what actually happens on
  quit, relaunch and upgrade (Claude sessions keep running and come back; Codex
  sessions end and resume on the next send; how to stop everything).

Out of scope: listing or killing tmux sessions that match no session row, sweeping
rows stuck in `spawning` or stale pending permission rows from a previous run, and
re-applying a changed tmux config to a running tmux server.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A migration adds the remembered pane to the session row; a store test shows it
      is written on bind and cleared on close.
- [x] A usecase test shows that at boot a session whose remembered tmux session is
      alive becomes open on that pane without any launch command being run, and its
      transcript is caught up from the stored cursor.
- [x] A usecase test shows that at boot a session whose remembered tmux session is
      gone stays closed and its remembered pane is cleared.
- [x] A usecase test shows that `open_session` on a session whose remembered tmux
      session is alive adopts it instead of launching `claude --resume`.
- [x] A test shows that when the hook endpoint changed since the previous run, a
      re-adopted session is marked as unable to deliver hooks, and that the web UI
      renders the notice for such a session (component test).

### Manual / on-hardware (verified by a human before merge)

- [ ] Launching the desktop app while it is already running focuses the existing
      window and starts no second server.
- [ ] With the desktop app running a Claude session mid-conversation, quit and
      relaunch the app: the session is listed as open, its terminal attaches to the
      same pane, output produced while the app was closed is shown, and the next send
      and its reply flow through hooks normally.
- [ ] After relaunching, `tmux -L io.github.x7c1.delta ls` shows no second Claude
      process for that session after sending to it.
