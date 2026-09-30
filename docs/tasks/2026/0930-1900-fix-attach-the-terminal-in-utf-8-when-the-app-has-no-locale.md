---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check"
assignee: null
branch: task/0930-1900-fix-attach-the-terminal-in-utf-8-when-the-app-has-no-locale
created_at: 2026-09-30T09:36:31Z
updated_at: 2026-09-30T11:39:41Z
---

# fix(pty): attach the terminal in UTF-8 even when the app has no locale

## Overview

In the desktop app launched from Finder (or a desktop file), the terminal
pane shows Japanese and other non-ASCII text as runs of `_`, and the
block characters of Claude Code's logo as broken shapes. The session itself
is fine — `tmux capture-pane` returns the right characters — only the
view is wrong.

The cause is the locale. A GUI app does not inherit a terminal's
environment, so the app process has no `LANG` / `LC_*`. The desktop shell
already imports `PATH` from the login shell at startup
(`backend/crates/apps/delta-app/src/login_path.rs`) but not the locale.
The PTY bridge (`backend/crates/apps/delta-server/src/pty.rs`) runs
`tmux -L <socket> attach-session` inside a pseudo-terminal with that
environment, and a tmux client with no UTF-8 locale assumes its terminal
cannot show UTF-8 and writes `_` for every wide character. Reproduced
directly: the same session attached with no locale prints
`delta ________________`; with `LANG=en_US.UTF-8` or with `tmux -u` it
prints `delta の動作確認中です`. `make dev` does not show it because it is
started from a terminal that has `LANG`.

### Change

1. **PTY bridge.** Attach with `tmux -u …`: the client's terminal is
   xterm.js, which is always UTF-8, so declaring it is always right and
   stops the symptom regardless of the environment. Keep `-u` next to the
   `-L <socket>` argument where the command is built, with a one-line
   comment saying why.
2. **Desktop shell.** Import the locale from the login shell together with
   `PATH`: `LANG` and any `LC_*` the login shell sets, through the same
   bounded login-shell call and pure parser (extend the marker-based
   output to carry several variables rather than running the shell twice).
   When the login shell sets no `LANG`/`LC_ALL`/`LC_CTYPE` either, set
   `LANG=en_US.UTF-8` so tmux, Claude Code, git and the other commands the
   sessions run all see a UTF-8 locale. Unit-test the parser and the
   fallback.
3. **Docs.** `docs/guides/development/local-run.md` "The desktop app": the
   `PATH` bullet becomes "`PATH` and locale", saying what is imported and
   the UTF-8 fallback.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] The PTY bridge's attach command includes `-u` (unit test on the
      built command line).
- [x] The login-shell import returns `PATH` and the locale variables from
      one call, and the fallback sets `LANG=en_US.UTF-8` only when no
      locale variable is present (unit tests).
- [x] `make check` is green.

### Manual / on-hardware (verified by a human before merge)

- [ ] macOS: `Delta.app` launched from Finder shows Japanese text and
      Claude Code's logo correctly in the terminal pane.

## Out of scope

- Any change to how the transcript or the conversation pane render text.
