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
branch: task/0930-1600-feat-give-the-macos-window-a-transparent-title-bar-in-the-page-theme
created_at: 2026-09-30T09:02:47Z
updated_at: 2026-09-30T09:58:36Z
---

# feat(app): give the macOS window a transparent title bar that takes the page's theme

## Overview

On macOS the desktop shell's window has a stock title bar: the three
traffic-light buttons on the left and the title "Delta" centred, painted in
the system's light or dark chrome. Delta's UI has its own themes (light,
dark, sepia; `<html data-theme>`, tokens in
`frontend/packages/apps/web/src/index.css`), so the bar sits above the page
in a colour that matches none of them. This task makes the title bar
transparent so the page's own background shows through it, with the
traffic lights floating over that background and no title text.

### Change

1. **Shell (`backend/crates/apps/delta-app/src/main.rs`, `open_window`).** On
   macOS only (`#[cfg(target_os = "macos")]`), build the window with
   `TitleBarStyle::Overlay` and `hidden_title(true)`: the bar becomes
   transparent, the page is laid out under it, the traffic lights are
   drawn over the page, and dragging and double-click zoom on the bar
   keep working natively. Add an `initialization_script` that sets
   `document.documentElement.dataset.shell = "tauri-macos"` before the
   page's scripts run, so the page can tell it is inside the macOS shell.
   Confirm the script does run for the external
   `http://127.0.0.1:<port>/` URL (Tauri injects initialization scripts on
   every navigation of the webview); if it does not, pass the marker
   another way (a query parameter on the URL the shell opens, read once
   and written to the same attribute) and say so in the docs. Linux and
   the browser are untouched: no attribute, no overlay.
2. **Page (`index.css` and the root layout).** When
   `html[data-shell="tauri-macos"]` is present, reserve the title-bar
   strip at the top of the viewport: a fixed-height inset (28 px, the
   height of a macOS title bar with hidden title; expose it as a CSS
   variable such as `--shell-top-inset`, 0 otherwise) painted with the
   page background token of the active theme, and push the app's root
   layout down by that inset. Anything sized to the viewport
   (`100dvh`/`100vh` heights, fixed or absolute overlays, the terminal
   column, dialogs and snackbars) must respect the inset so nothing is
   hidden under the traffic lights and nothing scrolls under the strip.
   The strip itself is the native bar, so it needs no drag handling and no
   pointer events. Do not draw a title or buttons of your own.
3. **Themes.** The strip must follow theme switches live (it reads the
   token, not a copy), and the traffic lights must stay legible on every
   theme: on the dark theme the buttons are coloured by macOS and need no
   help, but check that the light and sepia backgrounds do not wash out
   their outline; if they do, darken the strip slightly with a token
   rather than hard-coding a colour.
4. **Docs.** `docs/guides/development/local-run.md` "The desktop app": one
   sentence that on macOS the title bar is transparent and the page
   paints the strip, and how the page detects the shell
   (`data-shell="tauri-macos"`). Nothing in the install guide changes.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A frontend unit test renders the app root with and without
      `data-shell="tauri-macos"` on `<html>` and asserts the inset is
      applied only with it (the CSS variable or class the layout reads).
- [x] `make check` is green (the shell builds under `check-app-build` with
      the macOS-only window options behind `cfg`; Linux CI still compiles
      it).

### Manual / on-hardware (verified by a human before merge)

- [ ] macOS (`make app-dev` or `make app`): the title bar shows the page
      background in each of light, dark and sepia and follows a theme
      switch in Settings without a restart; the traffic lights are visible
      on all three; no title text; dragging the strip moves the window and
      double-clicking it zooms; nothing in the UI (navigator header,
      terminal column, dialogs, snackbars) is hidden under the strip.
- [ ] `make dev` in a browser and the Linux app are unchanged (no inset).

## Out of scope

- Custom window controls, `decorations: false`, or any change to the
  Linux window decorations (GTK draws them from the desktop theme).
- Traffic-light position tweaks beyond what `Overlay` gives.
