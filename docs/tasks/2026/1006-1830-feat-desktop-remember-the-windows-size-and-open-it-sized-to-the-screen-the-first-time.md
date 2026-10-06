---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && grep -q 'tauri-plugin-window-state' backend/crates/apps/delta-desktop/Cargo.toml && git grep -q 'min_inner_size' -- backend/crates/apps/delta-desktop/src && git grep -q 'StateFlags' -- backend/crates/apps/delta-desktop/src"
assignee: null
branch: task/1006-1830-feat-desktop-remember-the-windows-size-and-open-it-sized-to-the-screen-the-first-time
created_at: 2026-10-06T10:40:36Z
updated_at: 2026-10-06T11:27:03Z
---

# feat(desktop): remember the window's size and open it sized to the screen the first time

## Overview

`open_window` in `backend/crates/apps/delta-desktop/src/main.rs` builds the
window with a fixed `inner_size(1280.0, 800.0)` in logical pixels, no
minimum, no centring and no memory. On a large display the same small
window opens every launch and the user resizes it every time.

The window's size is the user's choice, and the app remembers it. The
first time, before there is a choice, the app picks a size from the screen
it opens on.

### Change

- **Remember size and maximized state** with `tauri-plugin-window-state`
  (`tauri-plugin-window-state = "2"`): register it after the
  single-instance plugin (`main.rs:76-91`, the comment there says why
  single-instance comes first) with `StateFlags::SIZE | StateFlags::MAXIMIZED`
  only. Position is left out on purpose: Wayland does not let an app place
  its window, so restoring a position would work on one platform and not
  the other. The plugin saves on close and restores on `build()`; it writes
  its state file under the app's config directory
  (`~/.config/<identifier>/` on Linux, `~/Library/Application Support/<identifier>/`
  on macOS — the identifier keeps the dev build's state apart from the
  installed app's). The erase action's follow-up task already removes that
  directory, so no change there.
- **First launch: size from the work area.** After `build()`, when the
  plugin restored nothing (no state file yet), read `window.current_monitor()`
  (falling back to `primary_monitor()`), take 80 % of the monitor's work
  area in logical pixels, clamp to the minimum below, `set_size` and
  `center()`. When no monitor can be read, fall back to the current
  1280×800. Put "work area → initial size" in a pure function with unit
  tests (a 1920×1080 area gives 1536×864; a tiny area is clamped to the
  minimum). Not maximized: on a large display a maximized window is too
  wide for a chat-and-terminal layout, and a user who wants it maximizes
  once and the state is remembered.
- **Minimum size**: `min_inner_size` at the smallest size the layout works
  at. Measure it against the SPA (the navigator plus the conversation and
  terminal panes side by side); record the number and how it was chosen in
  the code comment.
- The startup-order window task rewrites the same `open_window`; whichever
  lands second rebases.

### Docs

`docs/guides/development/local-run.md` "The desktop app": one bullet on the
remembered size and the first-launch rule, and where the state file lives.
`docs/guides/install/README.md` "Where the app keeps its data": add the
window state file to the list.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] The plugin is a dependency, the window has a minimum size, and the
      flags are restricted (gates in `check_command`).
- [x] Unit tests for the initial-size function, including the clamp and
      the no-monitor fallback.
- [x] `make check` passes and `make desktop-dev-build` builds.

### Before merge (verified outside the check command)

- [x] On macOS with the dev build: with no state file, the window opens
      centred at about 80 % of the work area. Resize it, maximize it, quit;
      relaunch restores the size and the maximized state. The installed app
      (no `.dev`) keeps its own state. A second launch while running only
      brings the window forward and does not resize it. The window cannot
      be shrunk below the minimum.
- [ ] On Ubuntu (GNOME Wayland): the same, and the window is placed by the
      compositor (no attempt to restore a position).
