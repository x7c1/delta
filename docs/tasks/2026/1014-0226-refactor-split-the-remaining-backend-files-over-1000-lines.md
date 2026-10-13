---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && make check-backend-file-length && grep -q 'check-backend-file-length' Makefile && grep -rq 'check-backend-file-length' .github/workflows/"
assignee: null
branch: task/1014-0226-refactor-split-the-remaining-backend-files-over-1000-lines
created_at: 2026-10-13T17:26:00Z
updated_at: 2026-10-13T18:18:37Z
---

# refactor(backend): split the remaining files over 1000 lines by responsibility, and gate the length

## Overview

Seven backend source files exceed 1000 lines. Each mixes several
responsibilities, which makes it hard to see what a change touches and to
review it:

| file | lines |
|---|---|
| `backend/crates/gateway/codex-agent/src/translate/mod.rs` | 1722 |
| `backend/crates/gateway/codex-agent/src/tests.rs` | 1593 |
| `backend/crates/gateway/codex-agent/src/adapter/mod.rs` | 1309 |
| `backend/crates/gateway/delta-wire/src/session_event.rs` | 1308 |
| `backend/crates/apps/delta-server/src/api/mod.rs` | 1039 |
| `backend/crates/gateway/git-worktree/src/git/mod.rs` | 1032 |
| `backend/crates/domain/delta-attribution/src/claude_format/mod.rs` | 1022 |

Split each into a directory module whose submodules each own one
responsibility, following the crate's existing layout (one type per module,
tests next to what they test or in a `tests/` directory split by behaviour).
Pick the seams from what the code does, not from line counts; a submodule
that ends near the limit is a sign the seam is wrong. This is a move, not a
rewrite: keep behaviour, keep each crate's public paths (re-export where an
item moves), and keep the existing doc comments with the items they
document, fixing any intra-doc link the move breaks.

So that the count does not creep back, add a `check-backend-file-length`
target to the `Makefile` that fails, naming each offender, when any `*.rs`
file under `backend/crates/` exceeds 1000 lines; generated files, if any
cross the limit, are excluded explicitly by path. Add it to `CHECK_STEPS`,
update the gate graph comment above `CHECK_STEPS`, run it in a job in
`.github/workflows/ci.yml` (it needs no build, so the backend job is fine),
and list it in `docs/guides/development/README.md` where the other backend
checks are listed.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `make check-backend-file-length` passes: no `.rs` file under
      `backend/crates/` exceeds 1000 lines.
- [x] `check-backend-file-length` is part of `make check` (`CHECK_STEPS`) and
      a CI workflow runs it.
- [x] `make check` passes (including `check-backend-doc`, so no intra-doc
      link broke in the move).

### Before merge (verified outside the check command)

- [x] `make check-backend-file-length` fails, naming the file, when one file
      is padded past 1000 lines locally.
- [x] The diff moves code without changing it: each split file's items are
      the same before and after, apart from `mod` / `use` lines and
      visibility needed by the move (reviewed with
      `git diff --color-moved=dimmed-zebra`).
