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
branch: task/1016-0105-raise-the-running-window-on-a-second-launch-under-wayland
created_at: 2026-10-15T16:05:00Z
updated_at: 2026-10-15T17:08:47Z
---

# fix(desktop): raise the running window on a second launch under Wayland

## Overview

Launching Delta while it already runs is caught by
`tauri_plugin_single_instance` in `backend/crates/apps/delta-desktop/src/main.rs`
(the callback calls `focus_running_window`). On macOS the existing window
comes to the front. On GNOME under Wayland it does not: the window stays where
it is and GNOME only shows a "Delta is ready" notification, because Wayland
compositors refuse to let a client take focus without an activation token
(focus-stealing prevention).

The launcher (GNOME Shell, a `.desktop` activation, a terminal using
xdg-activation) hands the *second* process an activation token in
`XDG_ACTIVATION_TOKEN` (and `DESKTOP_STARTUP_ID` on X11). The single-instance
plugin forwards only `argv` and `cwd` to the running instance (on Linux over
the session D-Bus), so the token is dropped and the running instance asks to
be focused without one.

Carry the token from the second process to the running instance and use it
when presenting the window:

- In the second process, read the token at the very start of `main`, before
  the plugin forwards the launch and exits. Find the least invasive way to get
  it to the first instance: for example pass it as an extra argument the
  callback recognises (if the plugin forwards the second process's real argv,
  re-exec is not an option, so check what it forwards), or a short-lived
  `0600` file in the runtime directory that the callback reads and deletes,
  or the plugin's own extension points. Explain the choice in a comment.
- In the running instance's callback, activate the window with that token
  (GTK: set the startup id / xdg-activation token on the window, then present
  it), falling back to today's focus request when there is no token.
- Remove `XDG_ACTIVATION_TOKEN` / `DESKTOP_STARTUP_ID` from the first
  instance's own environment at start-up as the xdg-activation spec asks, so
  child processes (tmux, agents) do not inherit a consumed token. Do this at
  the start of `main` before any thread starts (environment writes are unsafe
  once other threads run).
- Linux only; macOS behaviour must not change.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A unit test covers the token hand-off: given a second launch's
      environment with and without `XDG_ACTIVATION_TOKEN`, the running
      instance receives the token or `None`.
- [x] The token is not left in the first instance's environment (unit test of
      the start-up helper).
- [x] The desktop builds on Linux (`make check` runs `check-desktop-build`).

### Before merge (verified outside the check command)

- [x] On GNOME (Wayland), with Delta running and its window behind another
      window, launching Delta again from the app grid brings the existing
      window to the front with focus, and no "is ready" notification appears.
      Needs a person: whether the window actually came to the front is judged
      by eye on the desktop session.
- [x] Launching again from a terminal (`delta-desktop` or the dev binary)
      still focuses or at least does not break the running instance. Needs a
      person, as above.
