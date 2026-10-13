---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && [ $(wc -l < backend/crates/domain/delta-usecase/src/ports/session_store.rs 2>/dev/null || echo 0) -lt 600 ] && ! grep -q 'pub struct ThreadId' backend/crates/domain/delta-model/src/thread.rs && ! grep -q 'pub struct WireSessionListItem' backend/crates/gateway/delta-wire/src/rest/sessions_response.rs"
assignee: null
branch: task/1013-1513-refactor-split-oversized-modules-and-one-type-per-module
created_at: 2026-10-13T06:13:00Z
updated_at: 2026-10-13T14:00:48Z
---

# refactor(backend): split the oversized session-store port and end-to-end tests, and give each type its own module

## Overview

Pure structural changes; no behaviour changes.

### 1. Two files are too large to work in

- `backend/crates/domain/delta-usecase/src/ports/session_store.rs` (1276
  lines) defines the session-store port. Split it by concern — sessions,
  threads, messages, sends, and whatever other groups the trait's methods
  form — into a `session_store/` module, keeping the public path of the trait
  and its types unchanged for callers (re-export from the module root).
- `backend/crates/apps/delta-server/tests/end_to_end.rs` (1334 lines) holds
  many unrelated scenarios. Split it by subject into an integration-test
  module tree (`tests/end_to_end/main.rs` plus one file per subject, sharing
  the helpers through a common module), so each test keeps its name and still
  runs under `cargo test`.

### 2. One type per module

- `backend/crates/domain/delta-model/src/thread.rs` defines both `ThreadId`
  and `Thread`. Give each its own module, named after its type (`Thread`
  stays in `thread.rs`, `ThreadId` moves to `thread_id.rs`).
- `backend/crates/gateway/delta-wire/src/rest/sessions_response.rs` defines
  both `WireSessionListItem` and `WireSessionsResponse`. Give each its own
  module. The generated TypeScript bindings must not change (`gen-check`
  runs in `make check`).

Search the workspace for other modules that define more than one public
type in the same way and apply the same rule to them in this change; list
what was moved in the PR.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `ports/session_store.rs` no longer exists as a single file of 600 lines
      or more (gated in `check_command`), and callers compile unchanged
      against the same public paths.
- [x] `ThreadId` and `WireSessionListItem` moved out of `thread.rs` and
      `sessions_response.rs` into modules of their own (gated in
      `check_command`), and `make gen-check` reports no change to the
      TypeScript bindings.
- [x] The end-to-end tests are split by subject and the number of tests
      `cargo test -p delta-server --test end_to_end` runs is unchanged.

### Before merge (verified outside the check command)

- [ ] `cargo test -p delta-server --test end_to_end -- --list` lists the same
      test names before and after (compared in the PR).
