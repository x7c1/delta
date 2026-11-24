---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && ! grep -rqw civil_from_days backend/crates --include=*.rs && ! grep -rq 'SystemTime::now()' backend/crates/gateway/delta-sqlite/src backend/crates/apps/fake-claude/src backend/crates/apps/delta-server/src/comms_log backend/crates/apps/delta-server/src/api && ! grep -rqw jiff backend/crates --include=*.rs --exclude-dir=delta-time && ! grep -rqw jiff backend/crates --include=Cargo.toml --exclude-dir=delta-time"
assignee: null
branch: task/1125-0327-compute-hand-written-dates-and-times-with-delta-time
created_at: 2026-11-24T18:27:40Z
updated_at: 2026-11-24T18:52:09Z
---

# refactor(backend): compute hand-written dates and times with delta-time

## Overview

`backend/crates/libs/delta-time` wraps every date and time computation in
Delta's own types (`Instant`, `Date`) on top of `jiff`, but several places
still format or read times by hand:

- `delta-model/src/timestamp.rs` — `iso8601_utc(unix_secs)` formats a stored
  timestamp with a hand-written `civil_from_days` calendar conversion. It is
  called by `delta-sqlite/src/time.rs` (`now_iso8601`), by
  `delta-usecase/src/session_prune/prune_criteria.rs` (`PruneCriteria::cutoff`)
  and by `delta-usecase/src/interactor/storage/list_worktree_dirs.rs`
  (a directory's modification time).
- `fake-claude/src/transcript/timestamp.rs` — `rfc3339_now` /
  `rfc3339_from_unix` repeat the same calendar algorithm inline.
- `delta-server/src/comms_log/mod.rs` — `now_ms` reads `SystemTime::now()`
  for Unix milliseconds.
- `delta-sqlite/src/time.rs`, `delta-server/src/api/session_prune.rs` and
  `delta-server/src/api/storage_erase.rs` call `SystemTime::now()` to get the
  time that is formatted or compared (`prune_candidates`, `prune_sessions`,
  `erase_everything` take `now: SystemTime`, which reaches
  `PruneCriteria::cutoff` and `every_session_until` in
  `delta-usecase/src/interactor/routing/erase_everything.rs`).

Move these onto `delta-time`:

- Keep the function that decides the stored timestamp form in `delta-model`
  (`iso8601_utc`, or a replacement with the same role), and implement it with
  `delta-time`; delete the hand-written `civil_from_days` there and in
  `fake-claude`. The stored form is Delta's contract with its SQLite data and
  stays in `delta-model`, not in `delta-time`, which is a general library;
  `delta-model` (and `delta-usecase`) may depend on `delta-time` — a domain
  crate depending on a crate under `libs/` already happens
  (`delta-usecase` → `libs/release-signature`). Add to `delta-time` only the
  general operations this needs (for example an `Instant` from or to Unix
  seconds, subtracting a number of days, a second-precision RFC 3339 form),
  keeping `jiff` out of every public signature.
- **Do not change any output string.** Every stored timestamp keeps the
  second-precision form `YYYY-MM-DDTHH:MM:SSZ` (SQLite stores it as text and
  compares and orders existing rows against new ones), the fake transcript
  keeps its `YYYY-MM-DDTHH:MM:SSZ` form, and the comms log keeps Unix
  milliseconds. Pin each form with a test that asserts the exact string for
  known instants (including the existing `iso8601_utc` cases: `0`,
  `1_609_459_200`, `1_234_567_890`).
- Replace `SystemTime::now()` only where it produces the time that is
  formatted into a date/time string or compared against stored times: the
  call sites listed above, with the use case taking a `delta_time::Instant`
  for `now` instead of `SystemTime` where that is how the time reaches the
  cut-off. Leave alone the calls that only need a unique value or that set a
  file's mtime in a test (`tmux-driver/src/tmux/driver.rs`,
  `delta-server/src/claude_version.rs`,
  `delta-transcript/src/reader/find_session_transcript.rs`), and leave the
  `Workspace::modified_at` port returning the filesystem's `SystemTime`
  (convert it where it is formatted).
- Keep the behaviour at the edges as it is today: a time before the Unix epoch
  or a cut-off that underflows reads as the epoch (`1970-01-01T00:00:00Z`)
  rather than an error.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] No `civil_from_days` (or other hand-written calendar arithmetic) remains
      under `backend/crates`; `grep -rqw civil_from_days backend/crates`
      finds nothing (gate in `check_command`).
- [x] `jiff` is named only inside `backend/crates/libs/delta-time` (gate in
      `check_command`).
- [x] `SystemTime::now()` no longer appears in `delta-sqlite/src`,
      `fake-claude/src`, `delta-server/src/comms_log` or
      `delta-server/src/api` (gate in `check_command`).
- [x] Unit tests assert the exact stored form `YYYY-MM-DDTHH:MM:SSZ` for
      `0`, `1_609_459_200` and `1_234_567_890` Unix seconds, and that the
      prune cut-off for 31 days before 2021-01-01T00:00:00Z is
      `2020-12-01T00:00:00Z`.
- [x] A unit test asserts the fake transcript's timestamp form for known
      instants (`1970-01-01T00:00:00Z`, `2026-01-01T00:00:00Z`).
- [x] A unit test asserts that a cut-off reaching before the Unix epoch reads
      as `1970-01-01T00:00:00Z`.
