---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && ! grep -q 'repository_display_name:' backend/crates/gateway/delta-wire/src/rest/session_list_item.rs && ! grep -q 'repository_display_name:' backend/crates/gateway/delta-wire/src/rest/sessions_response.rs"
assignee: null
branch: task/1026-0531-share-the-session-listing-test-fixture
created_at: 2026-10-26T05:31:00Z
updated_at: 2026-10-26T06:36:18Z
---

# test(wire): share one session-listing fixture across the wire tests

## Overview

The inline tests of `backend/crates/gateway/delta-wire/src/rest/session_list_item.rs`
and `backend/crates/gateway/delta-wire/src/rest/sessions_response.rs` each
build a near-identical `delta_usecase::SessionListing`, spelling out every
field of the domain `Session` and `Thread`. Adding a field to either type
means editing both fixtures the same way, and they can drift apart.

### The change

Build the fixture in one place and use it from both test modules:

- Add a test-only helper module in `delta-wire` (gated by `#[cfg(test)]`,
  following the crate's existing conventions for shared test code; if there
  are none, follow the one-type-per-module and test-helper placement rules
  the perspectives apply) that returns a `SessionListing` with fixed,
  documented values, and lets a test override what it varies (for example
  the turn and the running subagents).
- Rewrite both tests to use it. Each test keeps asserting the same JSON it
  asserts today; only the construction moves.
- If other inline tests in `delta-wire` build the same domain `Session` or
  `Thread` field by field (for example in `src/session.rs`), use the shared
  helper there too when it fits without changing what those tests assert.

### Out of scope

- Any change to the wire types or their serialization.
- Test fixtures outside `delta-wire`.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] Neither `rest/session_list_item.rs` nor `rest/sessions_response.rs`
      builds a domain `Session` field by field (grep gate in the check
      command).
- [x] The wire tests assert the same JSON as before.
- [x] `make check` passes.
