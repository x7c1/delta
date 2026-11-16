---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: null
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check"
assignee: null
branch: task/1117-0303-move-between-sessions-with-ctrl-tab-on-desktop
created_at: 2026-11-16T18:03:45Z
updated_at: 2026-11-16T18:30:41Z
---

# feat(web): move to the next or previous session with Ctrl+Tab in the desktop app

## Overview

In the desktop app (Tauri), Ctrl+Tab focuses the session below the current one
in the session list, and Ctrl+Shift+Tab the one above it. The order is the order
the navigator shows: the rows `NavigatorPane` renders
(`frontend/packages/apps/web/src/features/navigator/NavigatorPane.tsx`, the
`rows` memo), which is the server's open-first order with the launches this
window tracks pinned on top (`launchesFirst`) and, while the pointer is over the
list, the order held by `heldOrder`. Moving focuses the target session the same
way a click on its card does (`setFocusedSession` in `useNavStore`, as
`SessionNode.tsx` does), and scrolls the virtualized list so the newly focused
card is visible.

Rules:

- **Desktop only.** Browsers reserve Ctrl+Tab and never hand it to the page, so
  the shortcut is bound only when the page runs inside the desktop shell
  (`isMacosShell()` or `isLinuxShell()` in
  `frontend/packages/apps/web/src/shell.ts`). No alternative key is added for
  the browser. Outside the shell, a Ctrl+Tab keydown that does reach the page is
  left alone (not prevented).
- **Delta takes the key before anything else in the page.** With focus in the
  xterm terminal (`features/terminal/TerminalPane.tsx`), in the composer or in
  any other input, Ctrl+Tab and Ctrl+Shift+Tab still move between sessions, and
  the key is not delivered to the terminal or the input (listen in the capture
  phase on `window`, `preventDefault` and stop propagation). Key matching uses
  `event.key === 'Tab'` with `ctrlKey` set and `altKey`/`metaKey` unset;
  `shiftKey` picks the direction. Key repeat moves one row per keydown.
- **Ends wrap around.** Ctrl+Tab on the last loaded row goes to the first row,
  Ctrl+Shift+Tab on the first row goes to the last loaded row (the same as tab
  switching in browsers and terminal emulators). Only the loaded rows count;
  the shortcut does not fetch further pages.
- **No focused session** (the new-session screen, or a focused id that is no
  longer in the list): Ctrl+Tab focuses the first row, Ctrl+Shift+Tab the last
  loaded row.
- **Empty list**: nothing happens and the key is still consumed in the shell.
- Every row counts regardless of its session's state: launching/pending
  launches pinned at the top, open sessions (idle or mid-turn, resuming), and
  closed sessions. Focusing is not an operation on the session itself, so no
  state changes or sends are triggered by the move.

This is the first app-wide keyboard shortcut of the web app. Put the matching of
the key and the computation of the target (current id + ordered ids + direction
→ target id) in a small pure module with unit tests, separate from the hook that
listens and focuses, so the next global shortcut has a place to follow.

Document the shortcut where the desktop app's behavior is described for users
(search `docs/guides/` for the desktop app's guide; add a short "Keyboard
shortcuts" note there if no such section exists).

## Acceptance criteria

### Automated (pipeline-verified)

- [x] Unit tests of the pure target computation cover: next and previous in
      the middle of the list, wrap-around at both ends, no focused session
      (forward → first, backward → last), a focused id missing from the list
      (same as no focus), a one-row list (stays on that row), and an empty list
      (no target).
- [x] Unit tests of the key matching accept Ctrl+Tab and Ctrl+Shift+Tab and
      reject Tab, Shift+Tab, Ctrl+Alt+Tab and Meta+Tab.
- [x] A component test renders the navigator with `data-shell="tauri-linux"`
      (and one with `tauri-macos`) and asserts that a Ctrl+Tab keydown focuses
      the next row in the rendered order, Ctrl+Shift+Tab the previous one, and
      that the event's `defaultPrevented` is true.
- [x] A test asserts that with the launches pinned above the server order
      (`launchesFirst`), Ctrl+Tab follows the rendered order, not the server
      order.
- [x] A test asserts that a Ctrl+Tab keydown dispatched on an element inside
      the terminal (or a textarea standing in for it) moves the focus and does
      not reach a keydown listener attached to that element.
- [x] A test asserts that without `data-shell` (browser) a Ctrl+Tab keydown
      changes no focus and is not `defaultPrevented`.

### Before merge (verified outside the check command)

- [ ] On the built desktop app on macOS and on Linux, Ctrl+Tab and
      Ctrl+Shift+Tab move between sessions in the list's order while the
      terminal pane has focus, and the terminal does not receive the key —
      needs a person: it needs the desktop app running on a real display and a
      physical keyboard, which no agent in this environment can reach.
