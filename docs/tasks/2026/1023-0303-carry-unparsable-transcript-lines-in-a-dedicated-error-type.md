---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, error-type-design]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && ! grep -rqE "TranscriptError::Parse|Error::Parse \{" backend/crates --include="*.rs"'
assignee: null
branch: task/1023-0303-carry-unparsable-transcript-lines-in-a-dedicated-error-type
created_at: 2026-10-23T06:34:14Z
updated_at: 2026-10-23T06:54:22Z
---

# refactor(server): carry unparsable transcript lines in a dedicated error type

## Overview

`TranscriptRead::unparsable_lines`
(`backend/crates/domain/delta-usecase/src/ports/transcript_read.rs`) is a
`Vec<TranscriptError>`, so the type admits any transcript error although only
`TranscriptError::Parse` ever goes in. Since a read no longer fails on an
unparsable line, `TranscriptError::Parse`
(`backend/crates/domain/delta-usecase/src/ports/transcript_error.rs`) and the
gateway's `Error::Parse`
(`backend/crates/gateway/delta-transcript/src/error.rs`) exist only to build
those entries: neither is ever returned as the error of a read.

Give the skipped lines their own type:

- Add a use-case error type for one unparsable line, carrying the transcript
  `path`, the 1-based `line` and the `serde_json::Error` as its `source`, with
  the same message as today's `TranscriptError::Parse`. Export it from
  `ports` next to `TranscriptError`.
- Make `TranscriptRead::unparsable_lines` a `Vec` of that type, and have the
  reader (`backend/crates/gateway/delta-transcript/src/reader/mod.rs`) build
  it directly.
- Remove `TranscriptError::Parse`, the gateway's `Error::Parse` and its
  conversion, now that nothing produces them.
- Update the fake transcript
  (`backend/crates/domain/delta-usecase/src/interactor/testing/fake_transcript.rs`),
  the tests, and the doc comments that name the removed variants.

`next_batch` keeps reporting each line at `warn` with `with_causes` after the
cursor write succeeds; the logged text must not change.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `TranscriptRead::unparsable_lines` holds the dedicated type, and the
      reader test still asserts the path, the line number and the
      `serde_json::Error` source of an unparsable line.
- [x] No `TranscriptError::Parse` or gateway `Error::Parse` remains (the
      `grep` in `check_command`).
- [x] The existing use-case test that reports an unparsable line once the
      cursor moves past it still passes.
