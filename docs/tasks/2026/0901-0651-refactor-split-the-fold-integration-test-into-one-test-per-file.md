---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && test -f backend/crates/domain/delta-attribution/tests/fold/main.rs && ! test -f backend/crates/domain/delta-attribution/tests/fold.rs && [ "$(grep -r "#\[test\]" backend/crates/domain/delta-attribution/tests/fold/ | wc -l)" -eq 48 ]'
assignee: null
branch: task/0901-0651-refactor-split-the-fold-integration-test-into-one-test-per-file
created_at: 2026-09-01T06:51:12Z
updated_at: 2026-09-01T08:14:20Z
---

# refactor(attribution): split the fold integration test into one file per test

## Overview

`backend/crates/domain/delta-attribution/tests/fold.rs` is a single flat file
of 1728 lines carrying 48 `#[test]` functions — the attribution-fold semantics
suite. It keeps growing with every attribution change (the `TaskOutput`
retrieval work added 3 tests / ~123 lines in one sitting), and it is the last
large flat test module on this seam: the sibling suite in `delta-usecase`
already keeps one test per file (`backend/crates/domain/delta-usecase/src/interactor/sync/tests/<test_name>.rs`),
so a fold test's file is currently found by scrolling while a usecase test's
file is found by name.

Convert the target to a directory module with **one test function per file,
named after the test**, mirroring the `delta-usecase` convention:

- `tests/fold.rs` becomes `tests/fold/main.rs`. `main.rs` keeps the existing
  module doc (`//! Attribution-fold semantics, ...`), the `support`
  declaration, and one `mod <test_name>;` per test file. Because the crate
  root moves into `tests/fold/`, the shared helpers need a path attribute:
  `#[path = "../support/mod.rs"] mod support;` — `tests/support/` itself does
  not move (it is also consumed by `tests/corpus.rs` and
  `tests/replay_properties.rs`, which are out of scope).
- Each of the 48 tests moves to `tests/fold/<test_name>.rs`, body
  byte-identical — this is a pure move, no assertion, name, or behavior
  change. Each file carries the `use` items its test actually needs
  (`use crate::support::*;` for the shared helpers, plus its
  `delta_attribution` / `delta_model` imports). One deliberate exception to
  byte-identity: a comment that referenced its neighbor by position ("Like
  the previous test, …" in
  `a_namespaced_local_command_name_line_matches_a_short_form_send`) loses its
  referent once tests live in separate alphabetically-ordered files, so it
  now names the referenced test instead — a comment-only change the move
  itself necessitated.
- A helper `fn`/`const` defined in `fold.rs` outside any test: if only one
  test uses it, it moves with that test; if several do, it moves into
  `tests/support/mod.rs` next to the existing shared helpers (do not create a
  second shared-helper module inside `tests/fold/`).

The test count is pinned at 48 by the check gate; `cargo test -p
delta-attribution` must report the same tests passing as before the move
(same names, same count). Run `make check` and fix whatever it reports.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `tests/fold/main.rs` exists, `tests/fold.rs` is gone, and the directory
      carries exactly 48 `#[test]` functions across its per-test files
      (gates appended to the check command).
- [x] The move is behavior-preserving: `make check` passes with the full
      attribution suite green (`cargo test -p delta-attribution` runs the
      same 48 fold tests by their unchanged names).
- [x] `tests/support/` stays in place and `tests/corpus.rs` /
      `tests/replay_properties.rs` still compile against it unchanged
      (covered by `make check`).

## Out of scope

- Splitting `tests/corpus.rs` or `tests/replay_properties.rs`, or moving
  `tests/support/` — this task only converts the `fold` target.
- Deduplicating the near-identical transcript-line helpers that exist in both
  `delta-attribution/tests/support/` and
  `delta-usecase/src/interactor/testing/` (a known, accepted duplication).
- Any change to a test's assertions, fixtures, or name; any production code
  change.
