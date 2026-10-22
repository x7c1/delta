---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, error-type-design]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && ! grep -q "skipping unparsable transcript line" backend/crates/gateway/delta-transcript/src/reader/mod.rs'
assignee: null
branch: task/1023-0302-report-unparsable-transcript-lines-once-the-cursor-moves-past-them
created_at: 2026-10-22T18:02:00Z
updated_at: 2026-10-22T18:36:47Z
---

# fix(server): report an unparsable transcript line once, after the cursor moves past it

## Overview

The transcript reader skips a line it cannot parse and logs a `warn` naming
the transcript and the line (`read_from` in
`backend/crates/gateway/delta-transcript/src/reader/mod.rs`). Its comment
says each line is reported once because the cursor moves past it. That holds
only when the cursor does move: `ClaudeConversationSource::next_batch`
(`backend/crates/domain/delta-usecase/src/interactor/sync/conversation_source.rs`)
advances it with `set_transcript_lines_read` after the read. When that store
write (or anything before it) fails, the cursor stays, the next tick reads
the same range again, and the reader warns about the same line again on
every 500 ms tick for as long as the store keeps failing.

The per-session suppression added for repeating transcript failures
(`note_transcript_failure` in `interactor/sync/poll_transcript.rs`) does not
cover this, because the reader logs the line itself and returns success.

Fix it by moving the report to where the cursor is advanced:

- The reader stops logging unparsable lines and returns them as part of the
  read: add a field to `TranscriptRead`
  (`backend/crates/domain/delta-usecase/src/ports/transcript_read.rs`)
  carrying each skipped line as a use-case error that names the transcript
  path and the 1-based line number and keeps the `serde_json::Error` as its
  `source` (the existing `TranscriptError::Parse` already carries these).
- `next_batch` reports them at `warn` (rendered with `with_causes`, with the
  session id) only after `set_transcript_lines_read` has succeeded, so a line
  is reported once per cursor advance past it. Lines skipped because the
  file is shorter than the cursor are not reported.
- Update the doc comments that describe where the warning comes from
  (`JsonlTranscript`'s type doc, `read_from`, `TranscriptRead`).

Keep the other skip paths (not UTF-8 at `debug`, blank, no-uuid) as they
are.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A reader test asserts an unparsable line is returned in the read with
      its path and line number and the `serde_json::Error` as its source,
      and that the lines around it are still read.
- [x] A use-case test with a fake store whose `set_transcript_lines_read`
      fails asserts no unparsable-line warning is emitted for that tick, and
      that once the store recovers the line is reported exactly once over
      several further ticks.
- [x] The reader no longer logs unparsable lines itself (the `grep` in
      `check_command`).
