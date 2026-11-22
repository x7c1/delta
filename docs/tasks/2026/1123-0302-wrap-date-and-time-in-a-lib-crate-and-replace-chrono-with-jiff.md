---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && (cd backend && cargo tree --workspace --target all -e normal,build,dev --prefix none > /dev/null) && ! (cd backend && cargo tree --workspace --target all -e normal,build,dev --prefix none | grep -q '^chrono ') && ! (cd backend && cargo tree --workspace --target all -e normal,build,dev --prefix none | grep -q '^url v2\\.5\\.[0-8] ') && ! grep -qw chrono backend/Cargo.toml && ! grep -rqw chrono backend/crates --include=*.rs --include=Cargo.toml && test -f backend/crates/libs/delta-time/Cargo.toml && ! grep -rqw jiff backend/crates --include=Cargo.toml --exclude-dir=delta-time && ! grep -rqw jiff backend/crates --include=*.rs --exclude-dir=delta-time"
assignee: null
branch: task/1123-0302-wrap-date-and-time-in-a-lib-crate-and-replace-chrono-with-jiff
created_at: 2026-11-22T18:02:00Z
updated_at: 2026-11-22T18:33:13Z
---

# build(backend): wrap dates and times in a delta-time crate and replace chrono with jiff

## Overview

RUSTSEC-2026-0543 marks `chrono` as unmaintained, and `cargo deny check
advisories` (`make audit`, the CI `audit` job) now fails on every pull request.
Only Delta's own crates depend on `chrono` (`cargo tree -i chrono` lists
`gh-cli`, `codex-agent` and `delta-desktop`), so the advisory is resolved by
removing the dependency, not by adding it to the `ignore` list in
`backend/deny.toml`. Do not add an `ignore` entry for it.

Add a new library crate, `backend/crates/libs/delta-time`, that owns every date
and time computation those crates need, and implement it on top of `jiff`
(which the advisory and chrono#1768 both suggest as the replacement). The
crate exposes Delta's own types (for example a UTC instant type and a calendar
date type); no `jiff` type appears in its public API, and no other crate
depends on `jiff` directly, so the implementation can be swapped later without
touching callers. `jiff` is already in `backend/Cargo.lock` as a transitive
dependency; add it as a workspace dependency used only by `delta-time`.
`crates/libs/*` is already in `members` and `default-members` of
`backend/Cargo.toml`.

Move the current `chrono` call sites onto `delta-time` and drop `chrono` from
`backend/Cargo.toml` (`[workspace.dependencies]`) and from the three crates'
manifests, so that no crate in the workspace builds `chrono` any more
(`cargo tree --workspace --target all` no longer lists it). `chrono` may stay
listed in `backend/Cargo.lock`: `tauri-utils` enables `serde_with` with its
default features, whose weak `chrono_0_4?/…` features make Cargo record the
optional dependency in the lockfile without ever building it, as it already
did for `jiff`:

- `backend/crates/gateway/gh-cli/src/gh.rs` — today's UTC date
  (`Utc::now().date_naive()`), subtracting `PR_FRESHNESS_DAYS` days
  (`freshness_cutoff`), and formatting the date as `YYYY-MM-DD` for the GitHub
  search query `updated:>=YYYY-MM-DD` (`search_query_for`); the tests build
  fixed dates.
- `backend/crates/gateway/codex-agent/src/adapter/mod.rs` — the current time
  as epoch milliseconds (`Utc::now().timestamp_millis()`).
- `backend/crates/gateway/codex-agent/src/content/build.rs` —
  `iso8601_from_epoch_ms`, epoch milliseconds to an RFC 3339 UTC string with
  millisecond precision and a `Z` suffix (`to_rfc3339_opts(SecondsFormat::Millis,
  true)`, e.g. `2026-03-07T01:02:03.004Z`), returning `None` for an
  out-of-range value.
- `backend/crates/apps/delta-desktop/src/logging/owner_only_appender.rs` —
  today's UTC date and its `YYYY-MM-DD` form in the daily log file name, which
  must keep matching the name `tracing-appender` gives the file.

The strings these produce must not change: the `YYYY-MM-DD` dates, and the
millisecond-precision `…Z` timestamps stored as `Message::created_at`
(SQLite compares and orders them as text). Pin each format with unit tests in
`delta-time` (including the zero-padding of single-digit months, days, hours
and milliseconds, and the out-of-range `None`), and keep the existing tests at
the call sites passing.

Out of scope: the hand-written date and time code elsewhere
(`delta_model::iso8601_utc`, `delta-sqlite/src/time.rs`,
`fake-claude/src/transcript/timestamp.rs`, `delta-server/src/comms_log`'s
`now_ms`, direct `SystemTime::now()` calls). A follow-up pull request moves it
onto `delta-time`; this one only replaces `chrono`, so the `audit` job passes
again.

While this was in progress a second advisory, RUSTSEC-2026-0592 (`url` below
2.5.9, reached through `reqwest` and `tauri`), started failing the same
`audit` job. Raise `url` to 2.5.9 or later in `backend/Cargo.lock` with
`cargo update -p url` (a semver-compatible lockfile change; no manifest
changes), so that this pull request's own `audit` job passes.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] No crate in the workspace builds `chrono` (`cargo tree --workspace --target all` gate), and `chrono` is gone from `backend/Cargo.toml` and every manifest and Rust source under `backend/crates/` (grep gates in `check_command`)
- [x] The workspace resolves `url` 2.5.9 or later (`cargo tree` gate in `check_command`)
- [x] `backend/crates/libs/delta-time` exists, and it is the only crate whose manifest or source names `jiff` (grep gates in `check_command`)
- [x] Unit tests in `delta-time` pin the `YYYY-MM-DD` date form, the millisecond RFC 3339 `…Z` form, subtracting days across a month and a year boundary, and `None` for out-of-range epoch milliseconds
- [x] The existing tests of `gh-cli` (`search_query_for`, `freshness_cutoff`), `codex-agent` (`iso8601_from_epoch_ms`) and `delta-desktop` (`the_file_name_is_the_appender_s`) pass unchanged in what they assert
- [x] `make check` passes, including `check-desktop-build`

### Before merge (verified outside the check command)

- [x] `cd backend && cargo deny check advisories` passes, reporting neither RUSTSEC-2026-0543 nor RUSTSEC-2026-0592 (needs network access; the CI `audit` job runs the same command)
