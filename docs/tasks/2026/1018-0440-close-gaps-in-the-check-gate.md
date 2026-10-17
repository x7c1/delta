---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && (umask 002 && cd backend && cargo test -p delta-update-helper)"
assignee: null
branch: task/1018-0440-close-gaps-in-the-check-gate
created_at: 2026-10-17T19:40:48Z
updated_at: 2026-10-17T21:45:19Z
---

# test: close three gaps in the check gate

## Overview

Three places where `make check` either fails for reasons unrelated to the
code, or passes without running a test that exists. Fixed together because
each one makes the gate say something about the code that is not true.

### 1. `delta-update-helper`'s `work_dir` tests fail under umask 002

`backend/crates/apps/delta-update-helper/src/work_dir.rs` refuses a working
directory whose parent the group or others may write to
(`WRITABLE_BY_OTHERS`). Some of its tests create their directories with the
process umask, so on a development machine with umask 002 (common on macOS
and on Linux systems with user private groups) three of them fail: the
directory they expect to be accepted comes out mode 775 and is refused as
designed. CI runs with umask 022, so it passes there. Reproduce first with
`umask 002 && cd backend && cargo test -p delta-update-helper`, find which
tests fail and why, and make each test set the modes it depends on
explicitly (e.g. `chmod` the directories it creates to the mode it means), so
the result no longer depends on the caller's umask. The production check
itself is correct and must stay as it is.

### 2. Tests behind `embed-web` are never run by the gate

`check-embedded` in the `Makefile` (and CI's "Build the embedded server"
step in `.github/workflows/ci.yml`) only *builds* `delta-server` with
`--features embed-web`. Tests gated on that feature never run anywhere; for
example
`allows_every_inline_script_of_the_embedded_build_by_its_hash` in
`backend/crates/apps/delta-server/src/app/static_web/index_page.rs`, whose
doc comment says so and tells the reader to run it by hand. Make
`check-embedded` also run `cargo test -p delta-server --features embed-web`,
make CI's step run the same tests (keeping `--locked`, as its neighbours do),
and update that doc comment and the comments around both steps so they
describe what now runs. Keep the frontend build a prerequisite, since the
feature embeds `dist/`. If running the whole `delta-server` test suite a
second time with the feature on is noticeably slow, say so in the report with
the measured time; do not narrow it with a test filter that a renamed test
would silently escape.

### 3. The resume backstop's re-adoption has no test of its own

When a session is opened and its row remembers a pane whose agent is still
alive, `open_session`
(`backend/crates/domain/delta-usecase/src/interactor/lifecycle/open_session.rs`,
the "Never-double-launch backstop", `adopt_remembered_pane_if_alive`) adopts
that pane through `adopt_live_pane` instead of resuming. Since the change
that rebuilds the running-subagent set on re-adoption, that path should also
restore the background subagents still in flight, but only boot re-adoption
is tested
(`interactor/lifecycle/tests/readoption_restores_the_running_background_subagents.rs`).
Add a test that reaches `adopt_live_pane` through the backstop in
`open_session` and shows that an outstanding background subagent launch is
restored to the running set (with its `thread_id`, `tool_use_id`, `task_id`,
`subagent_type`, `description` and `background`), and that a foreground one
is not. Follow the existing test's fixtures and the module's
one-test-per-file layout. If the test exposes a bug, fix it in this change.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `cargo test -p delta-update-helper` passes under umask 002 (the
      `check_command` runs it after `make check`).
- [x] `make check` runs `cargo test -p delta-server --features embed-web`
      through `check-embedded`, so
      `allows_every_inline_script_of_the_embedded_build_by_its_hash` runs in
      the gate.
- [x] A test opens a session whose remembered pane is alive, goes through the
      backstop in `open_session`, and asserts that an outstanding background
      subagent is restored to the running set and a foreground one is not.

### Before merge (verified outside the check command)

- [ ] The PR's CI run shows the embedded-server step running the
      `delta-server` tests with `embed-web`, and passing.
