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
branch: task/1007-1655-fix-desktop-stop-the-remembered-window-size-from-growing-on-linux
created_at: 2026-10-07T16:55:56Z
updated_at: 2026-10-07T18:17:44Z
---

# fix(desktop): stop the remembered window size from growing on Linux

## Overview

On Linux (GNOME on Wayland) the desktop app's remembered window size grows on
every launch, so after a few launches the window is taller than the screen
and has to be shrunk by hand each time. Measured on a 5120×2160 display at
125 % scaling with the dev build, launching and closing the window without
touching it, the state file's size went `2836×3496` → `2940×3674` (closed
with the window's close button) → `3044×3852` (closed with Ctrl-Q): +104
wide and +178 high physical pixels each time, the client-side decoration
(header bar and shadow) at a GTK scale of 2.

Cause: `tauri-plugin-window-state` (registered in
`backend/crates/apps/delta-desktop/src/main.rs`, wrapped by
`backend/crates/apps/delta-desktop/src/window_size.rs`) saves the size tao
reports and restores it with `set_size`. On Linux tao's reported size — both
the `Resized` event and `inner_size()` — comes from GDK's configure event,
which is the size of the whole toplevel including the client-side
decorations, while `set_size` is applied through `gtk_window_resize`, which
sets the content size. So each save adds the decoration and each restore
keeps it.

### What to change

- Remember and restore the window's **content** size, read the same way it
  is applied. On Linux read it from GTK (`gtk_window_get_size` on the
  window's `gtk::ApplicationWindow`, available through Tauri's
  `gtk_window()`; `gtk` is already a dependency), which uses the same units
  and meaning as `gtk_window_resize`. On macOS the plugin's size is already
  right; keep one mechanism for both platforms if that stays simple (Delta
  persisting its own logical content size, with the plugin kept for the
  maximized state only), or a Linux-only correction if that is clearly
  simpler — decide and say why in the module doc.
- Save the size whenever the app exits by any path Delta has: the window's
  close button, Ctrl-Q (`quit_shortcut.rs`), the erase flow's quit, and a
  server error's exit. Do not save while maximized or minimized (keep the
  last normal size, as the plugin does).
- **Clamp a restored size to the monitor** the window opens on: never larger
  than its work area less room for the decoration, and never below
  `MIN_SIZE`. On Wayland GTK reports no work area beyond the monitor
  geometry, so leave a margin for the desktop's own bars. This also covers a
  size saved on a monitor of another scale or size.
- A size already remembered by an affected version (too large) must come out
  fitting the screen on the next launch, through the clamp or by not reading
  the old values.
- Keep the first-launch sizing (80 % of the work area, `initial_size`) and
  the centring as they are.

### Out of scope

- Remembering the window position (Wayland does not let an app place its
  window).

## Acceptance criteria

### Automated (pipeline-verified)

- [x] The clamp is a pure function covered by unit tests: a size larger than
      the monitor is reduced to fit it with the decoration margin; a size
      within it is kept; each side is clamped on its own; the result never
      goes below `MIN_SIZE`; a size saved at another scale is converted
      before clamping.
- [x] Reading and writing the remembered size is covered by unit tests
      against a temp directory: a missing or unreadable file means a first
      launch; a saved size reads back unchanged; a maximized or minimized
      window does not overwrite the last normal size.
- [x] Every exit path named above saves the size (tests or a single shared
      exit hook whose callers are listed in the module doc).

### Before merge (verified outside the check command)

- [x] On this Linux machine (GNOME, Wayland, fractional scaling) with the dev
      build, starting the app and closing it without resizing leaves the
      remembered size unchanged, three times in a row, closing once with the
      close button and once with Ctrl-Q — needs a person to close the window.
- [x] Starting with the oversized size the affected build left behind, the
      window opens fitting the screen; after resizing it by hand and
      restarting, it opens at that size.
