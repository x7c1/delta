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
branch: task/1016-0102-withhold-the-secrets-before-the-desktop-starts-any-thread
created_at: 2026-10-15T16:02:00Z
updated_at: 2026-10-15T16:16:41Z
---

# fix(desktop): withhold the secrets from the environment before any other thread runs

## Overview

`withhold_secrets_from_children`
(`backend/crates/apps/delta-server/src/config/mod.rs`) calls
`std::env::remove_var` for `DELTA_AUTH_TOKEN` and `DELTA_HOOK_SECRET`. Its doc
comment already records the problem: the `delta-server` binary calls it before
its runtime starts a thread, but the desktop shell calls it from
`config_from_env_for` on its startup thread while the window's event loop
(and whatever threads GTK/WebKit and Tauri have started) already runs.
Modifying the environment while another thread may call `getenv` is undefined
behaviour in libc. The desktop normally has neither variable set, so the risk
is small, but the ordering is wrong and the fix is cheap.

Read both variables and remove them from the environment at the very start of
the desktop's `main` (`backend/crates/apps/delta-desktop/src/main.rs`), before
the Tauri builder, logging or any thread starts, and pass the values in to the
desktop's configuration entry point instead of having it read the process
environment for them. Keep the `delta-server` binary's behaviour identical (it
already withholds before any thread). Afterwards no code path calls
`remove_var` after a thread may have started; put that invariant in the doc
comments where the variables are read, and correct the comment that currently
describes the desktop ordering.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] The desktop configuration entry point takes the token and hook secret as
      arguments (or a small struct), and a unit test builds a desktop config
      from given values without touching the process environment.
- [x] `remove_var` for these variables is called only from the
      `delta-server` binary's pre-runtime path and the first statements of the
      desktop `main` (reviewable in the diff; `make check` builds the desktop).

### Before merge (verified outside the check command)

- [ ] `make desktop-dev` starts the desktop app and it loads the workspace
      as before. The merging session runs it on this Ubuntu machine and records
      the result in the PR.
