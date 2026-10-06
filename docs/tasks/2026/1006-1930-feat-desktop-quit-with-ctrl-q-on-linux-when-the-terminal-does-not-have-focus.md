---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && git grep -q 'connect_key_press_event' -- backend/crates/apps/delta-desktop/src && ! git grep -n '\\.menu(' -- backend/crates/apps/delta-desktop/src && git grep -qi 'ctrl-q\\|ctrl+q' -- docs/guides/install/README.md docs/guides/development/local-run.md"
assignee: null
branch: task/1006-1930-feat-desktop-quit-with-ctrl-q-on-linux-when-the-terminal-does-not-have-focus
created_at: 2026-10-06T11:38:32Z
updated_at: 2026-10-06T12:10:26Z
---

# feat(desktop): quit with Ctrl-Q on Linux when the terminal does not have focus

## Overview

On Linux there is no keyboard way to quit the desktop app. Tauri gives an
application menu with a Quit item only on macOS, where Cmd-Q works today
(confirmed on a real machine); on Linux the shell has no menu, no
accelerator and no key handling (`backend/crates/apps/delta-desktop/src/main.rs`
has no `.menu()`, `.on_menu_event()` or key code).

### Change

- **An invisible shortcut, no menu bar.** On Linux only, after the window is
  built, take its GTK window (`window.gtk_window()`) and
  `connect_key_press_event`; on Ctrl+Q (control modifier, keyval `q` or
  `Q`) call `app.exit(0)` and stop propagation. Add `gtk = "0.18"` (the
  version Tauri 2 builds on) as a Linux-only dependency next to `glib`. Do
  not add `Builder::menu()` on any platform: replacing macOS's default menu
  would drop its Edit submenu, and with it the WKWebView's Cmd-C/V/X/A.
- **Only when the terminal does not have focus.** The embedded terminal
  (xterm.js over the PTY bridge) forwards Ctrl-Q to the tmux pane, where it
  is XON; while the terminal is focused, Ctrl-Q must reach the pane and not
  quit. The handler therefore has to know whether the terminal is focused.
  Choose one of these and say in the PR why:
  - the page reports terminal focus to the shell — e.g. by setting the
    document title or a `data-` attribute the shell reads through
    `window.eval`, or by a tiny `tauri://localhost`-free channel such as a
    navigation the shell intercepts — keeping the "no Tauri IPC" rule of
    `main.rs:3-9`; or
  - the shell decides natively, e.g. by asking the webview for
    `document.activeElement` through `eval` on each Ctrl-Q, which is
    asynchronous and makes the handler's return value a guess.
  The first is likely the honest one. Whatever is chosen, the SPA side must
  be a few lines in the terminal component's focus/blur handlers, and the
  browser version must be unaffected.
- **Quitting goes the way closing the window goes**: the server stops with
  the process, Delta's tmux server and the Claude Code sessions in it keep
  running and are re-adopted on the next launch (`main.rs:36-41`). Nothing
  is saved or asked.
- Docs: `docs/guides/development/local-run.md` "Quitting" and
  `docs/guides/install/README.md` "Quitting, relaunching and upgrading" gain
  Ctrl-Q (Linux) beside Cmd-Q (macOS) and the terminal-focus exception; the
  `main.rs` module doc too.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] The GTK key handler exists, no menu is installed anywhere, and both
      guides name the shortcut (gates in `check_command`).
- [x] The focus-reporting side has a vitest (the terminal's focus and blur
      set and clear the signal) and the shell's decision ("Ctrl-Q with the
      terminal focused is not a quit") is a pure function with a unit test.
- [x] `make check` passes; `make desktop-dev-build` builds on Linux (CI's
      `check-desktop-build`).

### Before merge (verified outside the check command)

- [ ] On Ubuntu (GNOME Wayland, and X11 if available) with the dev build:
      Ctrl-Q quits when the page body or a text input has focus; with the
      terminal focused it does not quit and the pane receives it; after
      quitting, `tmux -L io.github.x7c1.delta.dev ls` still lists the
      session and the next launch re-adopts it; no menu bar appears.
- [ ] On macOS: Cmd-Q, Cmd-C/V/X/A in the page and in the terminal still
      work as before.
