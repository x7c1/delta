---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, error-type-design]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check"
assignee: null
branch: task/1018-0442-report-a-refused-transcript-once
created_at: 2026-10-17T19:42:00Z
updated_at: 2026-10-17T22:33:44Z
---

# fix(server): report a transcript the reader refuses once, not on every tail tick

## Overview

The transcript reader refuses some transcripts outright: one larger than
`MAX_TRANSCRIPT_BYTES` (`Error::TooLarge`) and one that is not a regular file
(`Error::NotRegularFile`), in
`backend/crates/gateway/delta-transcript/src/error.rs` and
`src/reader/mod.rs`. Such a refusal does not go away on its own: the same
file is refused again on the next read. But the background tail in
`backend/crates/apps/delta-server/src/state.rs` calls
`interactor.poll_transcript(TRANSCRIPT_POLL_INTERVAL)` every 500 ms and logs
any error as `warn!("transcript tail poll failed")`, so one oversized
transcript produces a warning twice a second for as long as the server runs.

The tail cannot tell a lasting refusal from a passing failure, because
`impl From<Error> for delta_usecase::Error` flattens every transcript error
into `delta_usecase::Error::Transcript(String)`
(`backend/crates/domain/delta-usecase/src/error.rs`), discarding the kind.

Fix:

- Give the use-case layer a vocabulary for transcript failures, so callers
  can tell at least: the transcript is refused (not a regular file, too
  large — with the values the messages need), an I/O failure, a parse
  failure, an aborted read. Map the gateway's errors onto it without
  rendering them to strings first, keeping the underlying cause available as
  a source. Follow how `delta_usecase::Error` already models other gateway
  failures; check every place that constructs or matches
  `Error::Transcript` (including how it is surfaced over HTTP, if it is) and
  keep their behaviour.
- Find where a per-session poll failure turns into the tick's error (in
  `poll_transcript` and below) and make a refused transcript reported once
  per session (at `warn`, naming the session and the reason), then stay
  quiet while the same refusal repeats. Report again if the refusal changes
  (a different kind, or a different size) or if the session recovers and
  later fails again, and log the recovery at `info`. One session's refused
  transcript must not hide another session's failures, and must not stop the
  other sessions from being polled. Passing failures (I/O, parse, aborted)
  keep their current reporting unless the code shows they have the same
  repeat problem; say which in the report.
- Decide where the "already reported" state lives (the session actor, the
  poll loop) by what the code already does for per-session state, and
  explain the choice in a comment.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A test converts each gateway transcript error into the use-case error
      and asserts its kind is preserved (refused-too-large,
      refused-not-a-regular-file, I/O, parse, aborted) rather than a string.
- [x] A test polls a session whose transcript is refused several times in a
      row and asserts the refusal is reported exactly once, then reported
      again after the session recovers and fails again.
- [x] A test shows a refused transcript in one session does not stop another
      session's transcript from being ingested on the same poll.

### Before merge (verified outside the check command)

- [ ] With `make dev`, a session whose transcript is replaced by a FIFO (or
      made larger than the limit with a sparse file) logs one warning for it in
      the server log over at least ten seconds, not one per tick.

## Last subagent report

(The completeness review's report, condensed in English.)

- `backend/crates/apps/delta-server/src/hooks/mod.rs` `internal_error` rendered errors with `err.to_string()`. With `Error::Transcript` now `#[error(transparent)]` and the cause kept as a `source`, a transcript I/O or parse failure reaching a hook (Stop, UserPromptSubmit, PreToolUse call `sync_transcript`) would lose its cause, e.g. `permission denied`, from both the log and the 500 body. The fix is to render `delta_usecase::with_causes(&err)` there; the file was outside the review's file set, so it was escalated.
- `collect_tick_replies`' `?` does not contradict the acceptance criteria: a refusal is caught in `sync_tick` and returns an empty batch, so it never reaches the `?`. What `?` still drops is a passing failure's tick result for the other sessions, unchanged from `main` and kept by the task's "keep their current reporting".
- Every other path that can carry a `TranscriptError` reaches `api_error.rs` or logs with `with_causes`.
