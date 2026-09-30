---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure]
max_refine_rounds: 2
retries_remaining: 1
check_command: "make check && [ \"$(find backend/crates/domain/delta-usecase/src/interactor/testing -name '*.rs' -exec wc -l {} + | grep -v ' total$' | sort -n | tail -1 | awk '{print $1}')\" -le 600 ] && ! test -f backend/crates/domain/delta-usecase/src/interactor/launch_options/tests.rs && [ \"$(wc -l < backend/crates/libs/delta-bootstrap/src/lib.rs)\" -le 300 ]"
assignee: null
branch: task/1001-0559-refactor-split-three-oversized-backend-modules-by-responsibility
created_at: 2026-09-30T20:59:07Z
updated_at: 2026-09-30T21:18:22Z
---

# refactor(backend): split three oversized modules by responsibility

## Overview

Three backend files have grown into single-file modules that are hard to
navigate. Split each into a directory module by responsibility. This is a pure
move: no behaviour, signature, or test assertion changes.

- **`backend/crates/domain/delta-usecase/src/interactor/testing/fake_store.rs`**
  (about 1230 lines). Most of it is one `impl SessionStore for FakeStore`
  block. Turn it into a `fake_store/` directory module: keep the `FakeStore`
  type and its construction/inspection helpers in `mod.rs`, and move the
  `SessionStore` methods into files grouped by the store's own areas
  (sessions, messages/transcript cursor, subagent launches, launch options,
  and so on — follow how the `SessionStore` port itself is organised). Rust
  allows several `impl` blocks of the same trait only once, so split with
  per-area inherent helper impls that the single trait impl delegates to, or
  whatever the repository already does for other large fakes — look for a
  precedent first.
- **`backend/crates/domain/delta-usecase/src/interactor/launch_options/tests.rs`**
  (700+ lines, one file). Split it into a `tests/` directory with one file per
  behaviour under test, the way the lifecycle tests are laid out in this
  crate.
- **`backend/crates/libs/delta-bootstrap/src/lib.rs`** (about 610 lines). It
  holds both `Config` and the composition root. Move `Config` (and its
  parsing/defaults) into its own module, and split the composition root by
  what it wires if it is still long, so `lib.rs` is mainly module declarations
  and re-exports. Keep the crate's public API (paths re-exported from the crate
  root) unchanged.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] No `.rs` file under `interactor/testing/` is longer than 600 lines
      (gated in `check_command`).
- [x] `interactor/launch_options/tests.rs` no longer exists as a single file
      (its tests live under `launch_options/tests/`).
- [x] `delta-bootstrap/src/lib.rs` is at most 300 lines.
- [x] The workspace builds and every existing test still passes unchanged
      (`make check`).
