---
status: completed
pipeline_phase: null
plan: null
follow_up_of: docs/tasks/2026/0912-1634-fix-announce-ingested-transcript-lines-per-session.md
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && ! grep -q "returns its events to the server tail loop" backend/crates/domain/delta-usecase/src/ports/async_event_sink.rs && ! grep -q "no live path emits on it yet" backend/crates/domain/delta-usecase/src/interactor/mod.rs && ! grep -q "Currently dormant" backend/crates/domain/delta-usecase/src/interactor/mod.rs'
assignee: null
branch: task/0912-1744-follow-up-fix-announce-ingested-transcript-lines-per-session
created_at: 2026-09-12T17:44:27Z
updated_at: 2026-09-12T18:09:20Z
---

# docs: describe the async event seam as live now that the tail emits on it

## Overview

Three doc comments still describe the interactor's async event seam as
dormant, or say the transcript poll hands its events back to the server tail
loop. Since the tail's per-session announcements, the Codex event pump, and a
deferred launch's outcome all emit on the seam, those comments mislead a
reader tracing how an event reaches the browser. Comment text only; no
behaviour changes.

- `backend/crates/domain/delta-usecase/src/ports/async_event_sink.rs`,
  lines 3-6 (the module doc's opening sentence, beginning
  `//! Every [\`SessionEvent\`] Delta produces today is *returned synchronously* to`).
  Replace those four lines with:

  ```
  //! Much of what Delta produces is *returned synchronously* to whoever drove
  //! the work — a hook handler returns its events to the HTTP handler, and that
  //! caller broadcasts them. That path stays exactly as it is.
  ```

- `backend/crates/domain/delta-usecase/src/interactor/mod.rs`, lines 230-232
  (the `event_sink` field doc, from `/// sink through [\`Interactor::with_event_sink\`]. Currently a dormant seam:`
  through `/// pump) that does lands in a later change.`). Replace those three
  lines with:

  ```
      /// sink through [`Interactor::with_event_sink`]. Several live paths emit on
      /// it: the Codex event pump, a deferred launch's outcome, and the
      /// background tail's per-session ingest announcements.
  ```

- `backend/crates/domain/delta-usecase/src/interactor/mod.rs`, lines 656-657
  (the `emit_async_event` doc, from `/// call. Currently dormant: the push-based producer that emits through it`
  through `/// (the Codex event pump) lands in a later change.`). Replace
  those two lines with:

  ```
      /// call. Live today: the Codex event pump, a deferred launch's outcome, and
      /// the background tail's per-session ingest announcements emit through it.
  ```

## Acceptance criteria

### Automated (pipeline-verified)

- [x] The seam's module doc no longer says the transcript poll returns its
      events to the server tail loop:
      `! grep -q "returns its events to the server tail loop" backend/crates/domain/delta-usecase/src/ports/async_event_sink.rs`.
- [x] The `event_sink` field doc no longer calls the seam dormant:
      `! grep -q "no live path emits on it yet" backend/crates/domain/delta-usecase/src/interactor/mod.rs`.
- [x] The `emit_async_event` doc no longer calls the seam dormant:
      `! grep -q "Currently dormant" backend/crates/domain/delta-usecase/src/interactor/mod.rs`.
