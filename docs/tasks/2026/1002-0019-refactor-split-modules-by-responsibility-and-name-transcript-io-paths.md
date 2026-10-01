---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure, error-type-design]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && git grep --untracked -l 'fn follow_relocated_transcript' -- backend/crates/domain/delta-usecase/src/interactor/hooks/ | grep -q 'follow_relocated_transcript' && ! git grep -q 'fn follow_relocated_transcript' -- backend/crates/domain/delta-usecase/src/interactor/hooks/hook_transcript.rs && [ -z \"$(find backend/crates/apps/fake-claude/src -name '*.rs' -exec wc -l {} + | awk '$2 != \"total\" && $1 > 500')\" ] && ! git grep -qE 'fn (recent_workdirs|cwd_exists|repository_clone_rows)' -- backend/crates/gateway/delta-sqlite/src/store/sessions.rs backend/crates/domain/delta-usecase/src/interactor/testing/fake_store/sessions.rs"
assignee: null
branch: task/1002-0019-refactor-split-modules-by-responsibility-and-name-transcript-io-paths
created_at: 2026-10-01T15:18:48Z
updated_at: 2026-10-01T15:39:40Z
---

# refactor(backend): split modules by responsibility and name the path in transcript IO errors

## Overview

Several backend modules have grown past a single responsibility, and the transcript
gateway drops the path from its IO errors. None of this changes behaviour; it makes
the code that the recent transcript-relocation and resume fixes touched easier to
read and its failures easier to diagnose.

### Module structure (one rule: a module holds one responsibility)

1. `follow_relocated_transcript` lives in
   `backend/crates/domain/delta-usecase/src/interactor/hooks/hook_transcript.rs`, but it
   is now called from both the hook path and the lifecycle (resume) path. Move it to
   its own module, `interactor/hooks/follow_relocated_transcript.rs`, with its helpers
   and tests, and leave `hook_transcript.rs` with the hook-admission logic.
2. `backend/crates/apps/fake-claude/src/transcript.rs` (~650 lines),
   `src/run.rs` (~670 lines) and `src/input/mod.rs` (~640 lines) are single files
   that mix several concerns (line shapes, the per-step driver, input parsing).
   Turn each into a directory module split along those concerns so that no file in
   `fake-claude/src` exceeds 500 lines. This crate is a test double that changes
   with every Claude Code drift fix, which is why it is worth the split.
3. The SQLite store's `backend/crates/gateway/delta-sqlite/src/store/sessions.rs` and
   the fake store's `backend/crates/domain/delta-usecase/src/interactor/testing/fake_store/sessions.rs`
   both mix row CRUD with history aggregation (`recent_workdirs`, `cwd_exists`,
   `repository_clone_rows`). Move those three into a sibling module with the **same
   name in both stores**, so the real store and the fake keep a one-to-one layout.

### Transcript IO errors carry the path

`delta_transcript::Error::Io` (`backend/crates/gateway/delta-transcript/src/error.rs`)
wraps a bare `std::io::Error`, so a failure logs as
`transcript io error: No such file or directory` with no way to tell which file or
directory it was. Give the IO variant the path it was operating on (the transcript
file, or the root being enumerated) and include it in the message; update the call
sites in `reader/` accordingly. Converting `delta_usecase::Error::Transcript(String)`
into a typed error is **out of scope**: no caller branches on the transcript error's
kind, so the string conversion stays.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `follow_relocated_transcript` is defined in its own module under
      `interactor/hooks/` and no longer in `hook_transcript.rs` (the two `git grep`
      gates in `check_command`).
- [x] No `.rs` file under `backend/crates/apps/fake-claude/src` exceeds 500 lines
      (the `find … | awk` gate).
- [x] `recent_workdirs`, `cwd_exists` and `repository_clone_rows` are defined in
      neither store's `sessions.rs` (the negative `git grep` gate), and both stores
      use the same sibling module name for them.
- [x] A delta-transcript unit test shows that an IO failure's message names the
      path that failed.

### Manual / on-hardware (verified by a human before merge)

- [ ] Reading the diff confirms the moves are behaviour-preserving (no logic
      changes beyond the error variant's new path field).
