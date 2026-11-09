---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: null
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && grep -qE '\"@xterm/xterm\": \"\\^6\\.' frontend/packages/apps/web/package.json && grep -qE '^  .@xterm/xterm@6\\.' frontend/pnpm-lock.yaml"
assignee: null
branch: task/1109-1331-upgrade-xterm-to-6
created_at: 2026-11-09T04:31:52Z
updated_at: 2026-11-09T04:51:02Z
---

# build(deps): upgrade xterm.js to 6

## Overview

Dependabot opened #602 to bump `@xterm/xterm` from 5.5.0 to 6.0.0 in `frontend/`. xterm.js 6 is a major release with breaking changes: `ITerminalOptions.overviewRulerWidth` moved under `ITerminalOptions.overviewRuler`, the viewport and scroll bar were reimplemented, and the addons are released in lockstep with the core. This task replaces #602 with a single change that bumps the core together with the addons the web app uses and adapts the code, based on the current `main`.

The terminal lives in `frontend/packages/apps/web/src/features/terminal/TerminalPane.tsx` (with `TerminalPane.test.tsx`), and `frontend/packages/apps/web/package.json` declares `@xterm/xterm`, `@xterm/addon-fit` and `@xterm/addon-unicode11`.

What to do:

1. **Bump `@xterm/xterm`** in `frontend/packages/apps/web/package.json` to `^6.0.0` (or the newest 6.x), and update `frontend/pnpm-lock.yaml` with `pnpm install` in `frontend/`
2. **Bump the addons to the versions released for xterm.js 6** — `@xterm/addon-fit` and `@xterm/addon-unicode11` — so that each addon's peer dependency (or the version the xterm.js 6 release names) matches the core. Check each addon's published `peerDependencies` rather than assuming
3. **Adapt the code to the 6.0 API** — fix every type error and every use of a removed or moved option, following the xterm.js 6.0.0 release notes. Keep the terminal's behaviour unchanged: input, output, resizing to fit the pane, Unicode 11 widths, and what happens when the session ends. Fix the cause of each error; do not silence it with `any`, `@ts-ignore` or `@ts-expect-error`
4. **Keep the tests meaningful** — update `TerminalPane.test.tsx` (and any other test or mock that stubs xterm.js) only where the API changed, without weakening what it asserts
5. **Check the bundle budget** — the web build enforces a bundle size budget; if xterm.js 6 changes the terminal chunk's size beyond it, report the measured size rather than raising the budget silently, and raise it only with the reason stated next to the budget
6. **Update the docs** that name the xterm.js version, if any

Out of scope:

- Adopting new xterm.js 6 features (synchronized output, the progress addon, ligatures)
- Other major dependency updates

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `make check` passes with xterm.js 6
- [x] `frontend/packages/apps/web/package.json` declares `@xterm/xterm` `^6.x`, and `frontend/pnpm-lock.yaml` resolves `@xterm/xterm@6.x`

### Before merge (verified outside the check command)

- [x] A person confirms the terminal still works end to end in a browser: run the server locally, open a session's terminal, type a command and see its output, resize the window and see the shell reflow, scroll back through output, and exit the shell and see the session end
