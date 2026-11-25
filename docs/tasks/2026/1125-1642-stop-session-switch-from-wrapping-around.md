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
branch: task/1125-1642-stop-session-switch-from-wrapping-around
created_at: 2026-11-25T07:42:24Z
updated_at: 2026-11-25T08:02:03Z
---

# fix(web): stop Ctrl+Tab / Ctrl+Shift+Tab from wrapping around the session list

## Overview

In the desktop app, Ctrl+Tab and Ctrl+Shift+Tab move the focus to the next or
previous session in the navigator's list
(`frontend/packages/apps/web/src/shortcuts/sessionSwitch.ts`,
`sessionSwitchTarget`). Today the ends wrap around: Ctrl+Shift+Tab on the first
row jumps to the last loaded row, and Ctrl+Tab on the last row jumps to the
first.

The list is ordered newest first, so the last loaded row is usually a long
idle, old session. Jumping there from the top has no use case and surprises
the user more than it helps. Stop wrapping:

- **Ctrl+Shift+Tab on the first row** stays on the first row.
- **Ctrl+Tab on the last loaded row** stays on the last row.
- **No focused session** (the new-session screen, or a focused id no longer in
  the list): both Ctrl+Tab and Ctrl+Shift+Tab focus the first row. Moving
  backward to the last loaded row would be the same jump to an old session
  this task removes.
- At an end, the key is still consumed (prevented and not propagated) in the
  shell, the same as with an empty list, so it never falls through to the
  terminal or an input. Since there is no move, `sessionSwitchTarget` may
  return `null` there or the current id; pick whichever keeps the hook simple,
  and make sure no focus change or scroll happens beyond a no-op.

Update the doc comment of `sessionSwitchTarget` and the user-facing note in
`docs/guides/install/README.md` (it currently says "ends wrap around").

## Acceptance criteria

### Automated (pipeline-verified)

- [x] Unit tests of `sessionSwitchTarget` cover: Ctrl+Tab on the last id stays
      on the last id, Ctrl+Shift+Tab on the first id stays on the first id,
      no focused session (and a focused id missing from the list) goes to the
      first id in both directions; the existing middle-of-list, one-row and
      empty-list cases still pass.
- [x] The component test `NavigatorPane.session-switch.test.tsx` replaces
      "wraps around at both ends" with a test that the focus stays on the last
      row on Ctrl+Tab and on the first row on Ctrl+Shift+Tab, and that both
      keydowns are `defaultPrevented`.
- [x] No remaining mention of wrapping around in the shortcut's code comments
      or in `docs/guides/install/README.md`.

### Before merge (verified outside the check command)

- [ ] On the built desktop app, Ctrl+Shift+Tab on the top session and Ctrl+Tab
      on the bottom loaded session leave the focus where it is — needs a
      person: it needs the desktop app running on a real display and a physical
      keyboard, which no agent in this environment can reach.
