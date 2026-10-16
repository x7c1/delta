---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && ! git grep -n 'scratch_workdir' -- backend/crates"
assignee: null
branch: task/1016-0101-require-a-working-directory-to-start-a-session
created_at: 2026-10-15T16:01:00Z
updated_at: 2026-10-15T18:14:23Z
---

# refactor(lifecycle): require a working directory to start a session

## Overview

The new-session screen can only start a session after the user picks a
repository, a directory or a pull request, but the backend still keeps a path
for a launch with none of them: `spawn_fresh`
(`backend/crates/domain/delta-usecase/src/interactor/lifecycle/spawn_fresh.rs`)
falls back to `scratch_workdir_for` (`lifecycle/scratch_workdir_for.rs`), an
empty per-launch directory under the data directory's `sessions/`, and the
launch plan carries a `scratch_workdir` flag that `launch_prep.rs` uses to
create it. Nothing in the product reaches this path any more; it only costs
code, tests and documentation.

Remove it end to end:

- Make the working directory required for a fresh spawn at the API boundary.
  A request with neither a repository/worktree nor a directory is refused with
  a `400` whose message says a working directory is required, before any row
  is written or any launch starts. Keep the wire type and the generated
  TypeScript types honest (if the field is optional only because of this
  fallback, make it required and run `make gen`).
- Delete `scratch_workdir_for`, the `scratch_workdir` flag and its branch in
  launch preparation, `session_workdir_base` if nothing else uses it, and the
  tests that pin the scratch behaviour
  (`lifecycle/tests/a_scratch_workdir_that_cannot_be_created_launches_no_pane.rs`,
  `a_codex_launch_without_a_repository_creates_its_scratch_workdir.rs`,
  `a_launch_without_a_repository_creates_its_scratch_workdir_before_the_pane.rs`).
  Replace them with a test of the new refusal.
- Stop creating the data directory's `sessions/` subdirectory, and update every
  document that describes it or the scratch launch — at least
  `docs/guides/security.md` (the `sessions/` 0700 lines) and the install guide;
  `git grep -n "sessions/"` and `git grep -n -i scratch` across `docs/` and
  the READMEs to find the rest. An existing `sessions/` directory left by older
  versions is not migrated or deleted (out of scope: it is empty-or-harmless
  user data; the Settings → Storage view may keep listing what it finds).
- Update the fakes and end-to-end suites that start sessions without a
  directory (for example the fake-codex `full_loop` support passes
  `scenario.dir.join("sessions")`; `e2e-fake` and the API mocks may do the
  same) so they pass an explicit directory instead.

Per the session-state matrix in the task-authoring notes, this changes only
the *spawn* operation, so the relevant states are those of a brand-new
session; resuming, opening, closing and sending to an existing session (whose
workdir is already recorded) are out of scope and must keep working unchanged.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `git grep -n 'scratch_workdir' -- backend/crates` finds nothing
      (appended to `check_command`).
- [x] A test posts a fresh-spawn request with no repository, worktree or
      directory and gets `400` with no session row created and no pane
      launched.
- [x] Spawns with a directory, with a repository worktree, and from a pull
      request still pass their existing tests.
- [x] The generated wire types are in sync (`make check` runs `gen-check`).

### Before merge (verified outside the check command)

- [x] `make dev`: starting a session from a directory and from a repository
      still works end to end in the browser. The merging session runs it and
      records the result in the PR.
