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
branch: task/1015-1057-end-option-parsing-keep-hook-secret-out-of-argv-and-stop-orphaned-hooks
created_at: 2026-10-15T01:57:09Z
updated_at: 2026-10-15T03:26:16Z
---

# fix(launch): end option parsing before user text, keep the hook secret out of argv, and stop sessions whose hooks point at a port Delta no longer holds

## Overview

A set of small corrections where text Delta passes to another program is
interpreted more broadly than intended, or where Delta's hooks can keep
talking to a port it no longer owns.

### Arguments

1. **First prompt.** `domain/delta-usecase/src/interactor/lifecycle/spawn_fresh.rs`
   (and the same shape in `gateway/claude-agent/src/lib.rs`) appends the first
   prompt as the last positional argument of `claude` without `--`, so a
   prompt starting with `-` is read as an option (a Markdown list item such as
   `- fix the build` already makes the spawn fail). Put `--` before the prompt
   if `claude` accepts it (check with the installed `claude --help` and a quick
   run); otherwise do not pass a prompt that starts with `-` on the command
   line and send it after launch the way later prompts are sent. Paths under
   `backend/crates/`.
2. **`open-cwd`.** `domain/delta-usecase/src/interactor/open_cwd/mod.rs` and
   `gateway/external-opener/src/lib.rs` pass the path to `code` without `--`.
   Refuse a path that is not absolute, and pass `--` before it.
3. **tmux start directory.** `gateway/tmux-driver/src/tmux/commands.rs` passes
   the workdir to `new-session -c`, which tmux expands as a format, so `#(`
   in a directory name runs a command. Escape `#` as `##` in the value (and
   check whether tmux 3.x needs anything else escaped there), with a test that
   a directory named `a#(touch x)b` is used literally.

### Hook secret

4. `libs/delta-bootstrap/src/settings.rs` renders the hook secret (`hs`) into
   the `curl` command lines of the SessionStart and statusLine command hooks,
   so it appears in the process list on every status-line refresh, unquoted,
   and `apps/delta-server/src/config/hook_state/mod.rs` adopts
   `DELTA_HOOK_SECRET` or the persisted file without checking its form.
   - Accept only a lowercase hex secret of the expected length when adopting
     it; refuse to start otherwise, naming the source.
   - Keep the secret out of argv: the command hooks read the URL (or a header)
     from a `0600` file in the session settings directory (for example
     `curl -K <file>`), and every value interpolated into a shell command is
     quoted.
   - Remove `DELTA_HOOK_SECRET` from the environment children inherit, as the
     auth token already is.

### Port taken over

5. When the desktop shell finds its recorded port taken and moves to another
   one (`apps/delta-server/src/serve.rs`), sessions that survived in tmux
   still post their hooks to the old port, where another process now
   listens. On such a port change, Delta ends those sessions' panes and marks
   them closed with a reason the UI shows ("Delta moved to a new port; resume
   to reconnect"), so resuming relaunches them with hooks pointing at the new
   port. A start on the recorded port changes nothing.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A test spawns with a first prompt `- fix the build` and asserts the
      argv either has `--` before it or carries no prompt (and the prompt is
      sent after launch).
- [x] Tests: `open-cwd` refuses a relative path and passes `--` before an
      absolute one.
- [x] A test asserts the tmux `-c` value for `a#(touch x)b` is escaped.
- [x] Tests: a non-hex or wrong-length `DELTA_HOOK_SECRET` or persisted secret
      is refused with a message naming its source; the rendered session
      settings contain no hook secret in any `command` string; the secret file
      is `0600`; `DELTA_HOOK_SECRET` is absent from the child environment.
- [x] A test: starting on a different port than the recorded one ends the
      surviving sessions and marks them closed with the reason; starting on
      the recorded port leaves them running.
- [x] `make check` passes.

### Before merge (verified outside the check command)

- [ ] With `make dev`, a new Claude session started with the first message
      `- reply with ok` launches and receives that message; the status line
      still updates, and no `curl` command line visible in the process list
      carries the hook secret.
