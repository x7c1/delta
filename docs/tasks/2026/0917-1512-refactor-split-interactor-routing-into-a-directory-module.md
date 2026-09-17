---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && test ! -e backend/crates/domain/delta-usecase/src/interactor/routing.rs && test -f backend/crates/domain/delta-usecase/src/interactor/routing/mod.rs && ! grep -rq 'mod test_seams' backend/crates/domain/delta-usecase/src/ && [ \"$(find backend/crates/domain/delta-usecase/src/interactor/routing -name '*.rs' -exec wc -l {} + | awk '$2!=\"total\" && $1>500' | wc -l)\" -eq 0 ]"
assignee: null
branch: task/0917-1512-refactor-split-interactor-routing-into-a-directory-module
created_at: 2026-09-17T06:12:03Z
updated_at: 2026-09-17T06:59:27Z
---

# refactor(usecase): split `interactor/routing.rs` into a directory module

## Overview

`backend/crates/domain/delta-usecase/src/interactor/routing.rs` is the
interactor's public surface — every use case that reaches a session's actor.
It has grown to about 1,250 lines with 29 public methods plus a 300-line
test-only `test_seams` module, and every new actor input adds to it. The file
already marks its own seams: seven `// ---- <name> ----` dividers group the
methods by responsibility (API commands, runtime-state queries, hook
deliveries, permission decisions, question answers, send cancellation,
background ticks).

Turn the file into a directory module along those seams. This is a **move**:
no behaviour, signature, visibility-as-seen-from-outside-the-module, or log
line changes.

### Change

- `interactor/routing.rs` becomes `interactor/routing/mod.rs` plus one file
  per responsibility, each holding an `impl Interactor` block with the
  methods of that group and the doc comments they carry today:
  `commands.rs`, `queries.rs`, `hooks.rs`, `permissions.rs`, `questions.rs`,
  `send_cancellation.rs`, `ticks.rs` (adjust a name if the content reads
  better under another, but keep one file per existing divider).
- `mod.rs` keeps the module doc, the `mod` declarations, and what every group
  shares: the `request` / `query` helpers and `mint_session_id` (widen them to
  `pub(super)` as needed — not further). `collect_tick_replies` is used only
  by the ticks, so it moves with them.
- The grain is the responsibility group, not the single method: most methods
  here are five-line forwards to an actor input, and one file per forwarder
  would scatter a surface that is read as a list. A group file that is itself
  large or mixes concerns may be split further where that clearly helps.
- The test-only `test_seams` module becomes `routing/testing.rs` (or a
  `testing/` directory if it splits naturally), declared `#[cfg(test)]` from
  `mod.rs` — the conventional name for a module of test helpers, and what the
  other modules of this crate use. Its contents are unchanged.
- The `// ---- name ----` dividers disappear with the file they divided; each
  group's explanatory comment block (several dividers are followed by a
  paragraph explaining the group) becomes that file's `//!` module doc.
- Fix what the move breaks: intra-doc links and prose references that named
  `routing.rs` or a line in it (grep the repo, including `docs/`), and imports
  that each new file needs narrowed to what it uses.

### What must not change

- The public API of `Interactor`: every method keeps its name, signature,
  visibility and doc comment. `cargo doc` for the crate lists the same items.
- No test is added, removed or rewritten other than for its `use` paths.
  The number of tests in the crate is the same before and after; record the
  count from `cargo test -p delta-usecase -- --list` on `main` before editing
  and compare.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `interactor/routing.rs` no longer exists and `interactor/routing/mod.rs`
      does (`check_command` tests both).
- [x] No file under `interactor/routing/` exceeds 500 lines
      (`check_command` fails if one does).
- [x] No `mod test_seams` remains in the crate (`check_command` greps).
- [x] The workspace builds, and every existing test passes unchanged, under
      `make check` (which also runs clippy with `-D warnings` and rustdoc link
      checks where configured).

### Manual / on-hardware (verified by a human before merge)

- [ ] Reading the diff confirms it is a move: method bodies and doc comments
      are unchanged apart from paths and the divider-to-module-doc conversion.
