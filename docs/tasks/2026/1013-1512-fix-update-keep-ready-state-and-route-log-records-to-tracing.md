---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, error-type-design]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && ! grep -rn 'Version::new(0, 0, 0)' backend/crates/domain/delta-usecase/src/release_check/"
assignee: null
branch: task/1013-1512-fix-update-keep-ready-state-and-route-log-records-to-tracing
created_at: 2026-10-13T06:12:00Z
updated_at: 2026-10-13T12:01:06Z
---

# fix(update): restore a verified download after restart, route `log` records to tracing, and tighten release-check types

## Overview

Four defects around the in-app update and release check.

### 1. A verified download is forgotten on restart

The desktop app downloads a newer release into `updates/` and verifies it
before offering Install. That "ready" state lives only in memory: after a
restart, the footer shows Update again although the verified file is still in
`updates/`, and pressing it downloads the same release again. At startup,
recognise a file in `updates/` that matches the newest release offered,
verify it again (the user owns `updates/`, so a file found there is not
trusted until re-verified — keep that rule), and restore the ready state.
Delete a leftover file that does not match or fails verification. Cover it
with tests for: matching and valid, matching but corrupt, for an older
release, and no file.

### 2. `log` records never reach tracing or the log file

Delta logs through `tracing`, but nothing bridges the `log` crate, so any
dependency that logs through `log` is invisible — in the terminal and in the
desktop app's log file. One known case: on Linux, `rustls-platform-verifier`
(`load_native_certs()`, reached from `GithubReleaseFeed::new` during
`AppState::build`) warns through `log` about certificates it cannot read.
Install the `log` → `tracing` bridge (`tracing-log`, or the subscriber's
built-in support) where each binary initialises its subscriber
(`delta-server`, `delta-desktop`), so these records follow the same filter
and outputs. Cover it with a test that a `log::warn!` reaches the configured
subscriber.

### 3. `ClientBuildError` renders its cause twice in a chain

`backend/crates/gateway/release-feed/src/client_build_error.rs`: the
`Display` text embeds the cause (`{0}`) while `#[from]` also exposes it as
`source()`. A renderer that walks the chain prints the cause twice; the
current call site (`delta-bootstrap`'s release check, `warn!(error = %err)`)
prints only the top level, which is why it reads correctly today. Make
`Display` describe only this level, and make the call site render the whole
chain (follow the existing chain-rendering convention in the codebase if one
exists; `with_causes` in the usecase crate is private — expose or reuse it
rather than duplicating it). Check the other error types in `release-feed`
and the release-check path for the same pattern and fix them alike.

### 4. `ReleaseCheck::disabled()` invents a version

`backend/crates/domain/delta-usecase/src/release_check/mod.rs`: the disabled
check sets `current: Version::new(0, 0, 0)`, a value that is never read
because there is no feed. Represent "disabled" in the type instead (for
example, an enum or an `Option` holding the feed and the current version
together), so no placeholder version exists.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] Tests cover restoring the ready state at startup for a matching valid
      file, and discarding a corrupt, outdated or absent one.
- [x] A test shows a `log::warn!` record reaching the tracing subscriber the
      binaries install.
- [x] `ClientBuildError`'s `Display` no longer embeds its source, and a test
      shows the rendered chain contains the cause exactly once.
- [x] `ReleaseCheck` has no `Version::new(0, 0, 0)` placeholder (gated in
      `check_command`).

### Before merge (verified outside the check command)

- [x] On this Mac, with a newer release available: download it, quit Delta,
      start it again — the footer offers Install without downloading again.
