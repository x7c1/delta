---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && grep -q "launch_prep_deadline" backend/crates/domain/delta-usecase/src/launch_config.rs && grep -q "DELTA_LAUNCH_PREP_DEADLINE_MS" backend/crates/apps/delta-server/src/main.rs && test -f backend/crates/domain/delta-usecase/src/interactor/lifecycle/tests/a_worktree_that_landed_off_its_planned_path_fails_the_launch.rs && test -f backend/crates/domain/delta-usecase/src/interactor/lifecycle/tests/a_launch_preparation_that_outruns_its_deadline_reports_spawn_failed.rs'
assignee: null
branch: task/0827-0343-fix-launch-prep-planned-path-and-deadline
created_at: 2026-08-27T03:43:14Z
updated_at: 2026-08-27T04:33:00Z
---

# fix(sessions): fail a launch whose worktree landed off its planned path, and make the preparation deadline configurable

## Overview

Two loose ends of the accept/launch split in the Claude spawn path
(`backend/crates/domain/delta-usecase/src/interactor/lifecycle/`), both inside
`launch_prep.rs` and its configuration. Neither changes the behaviour of a
healthy launch.

### 1. A worktree that landed somewhere else must fail the launch, not warn

`spawn_fresh` plans the launch directory at accept time
(`plan_worktree_launch_dir` in `worktree_launch_dir.rs`) and stores it as the
session row's `cwd` and as `LaunchingSpawn.workdir`. For a
`WorktreeStartPoint::UseRemoteBranch(name)` start point the plan is "reuse the
worktree already holding `name`, else `<worktree_base>/<slug>-<id>`". The launch
task then runs `resolve_worktree_launch_dir`, which makes the same decision
again — and can reach a different answer when a worktree for that branch
appeared between accept and launch. That is not hypothetical: since the
accept/launch split, a user can start a second new session from the same PR
while the first is still checking out; the second plan sees no worktree for the
branch yet, the second build finds the first session's worktree and returns
*its* path without creating anything at the planned path, and `prepare_launch`
(`launch_prep.rs`, the `if built != launching.workdir` arm) only logs a warning
before launching tmux in the planned — nonexistent — directory. The session
row already says `cwd = <planned>`, so nothing about it is recoverable from the
UI: the card sits `Starting` until the bind watchdog reaps it with no reason.

Make the mismatch a launch failure. Concretely:

- In `prepare_launch`, when the built path differs from `launching.workdir`,
  return an error instead of warning. Add a dedicated `Error` variant in
  `delta-usecase/src/error.rs` (e.g. `WorktreeLandedElsewhere` — pick a name in
  the style of its neighbours) whose `Display` names the branch, the planned
  path and the path the worktree actually landed on, so the `spawn_failed`
  `reason` the browser shows says what happened and where the branch is. Map
  it in `delta-server/src/api/api_error.rs` beside `LaunchPreparationTimedOut`
  (it never reaches a response body either, but the match must stay
  exhaustive). Because `finish_launch` already rolls back any `Err` from the
  preparation (row deleted, `SpawnFailed { reason: Some(..) }` emitted), no
  change is needed there beyond doc drift.
- Do not try to "fix up" the launch by using the built path: `cwd` is already
  persisted, and git forbids one branch checked out in two worktrees, so
  neither re-pointing the session nor building at the planned path is
  available. Fail fast; a Retry from the failed chip re-plans and, now that the
  worktree exists, lands on it (the existing `UseRemoteBranch` reuse rule).
- Only `UseRemoteBranch` can diverge — `Head`/`RemoteBranch` always build at
  `default_worktree_path` — so the check may stay where it is, or move into
  the build by handing `resolve_worktree_launch_dir` the planned path; either
  is acceptable, but keep the adapter-backed (Codex) caller in
  `spawn_adapter_session.rs` working unchanged (it has no plan phase).
- Update the module and method docs that currently describe the mismatch as
  "logged loudly" (`launch_prep.rs` `prepare_launch` doc, step 1;
  `worktree_launch_dir.rs` `plan_worktree_launch_dir` doc; `finish_launch.rs`
  rollback list; `docs/guides/api/sends.md` where it lists what can fail the
  background preparation) so they describe the new outcome.

### 2. `LAUNCH_PREP_DEADLINE` belongs in `LaunchConfig`, and its timeout path needs a test

Every other launch watchdog (`pending_spawn_deadline`, `resume_ready_deadline`,
`echo_deadline`, `permission_decision_deadline`) lives in
`delta-usecase/src/launch_config.rs` with a production default and an
environment override in `delta-server/src/main.rs` (`launch_from_env`:
`DELTA_LAUNCH_DEADLINE_MS`, `DELTA_ECHO_DEADLINE_MS`, …), so a test or the
e2e-fake fixture can shrink it. The preparation deadline is the exception:
`LAUNCH_PREP_DEADLINE` is a `const` in `launch_prep.rs`, passed straight into
`tokio::time::timeout`, and its timeout arm (`Error::LaunchPreparationTimedOut`)
has no test at all.

- Add `launch_prep_deadline: Duration` to `LaunchConfig` (doc it like its
  siblings; default = the existing 10-minute constant — keep the constant's
  rationale doc, wherever it ends up living) and extend
  `default_matches_the_production_constants`.
- `spawn_launch_preparation` reads the deadline from `core.launch` instead of
  the constant.
- `launch_from_env` gains `DELTA_LAUNCH_PREP_DEADLINE_MS` (same parse shape as
  the others) and its doc-comment bullet. Mention the variable where
  `docs/guides/api/sends.md` describes the 10-minute give-up, the way that
  guide already names `DELTA_ECHO_DEADLINE_MS` for the echo watchdog.

### Tests

Usecase tests in `lifecycle/tests/`, one per file, registered in `tests/mod.rs`
(alphabetical like the siblings). Both drive the interactor through the fakes
in `interactor/testing/` and observe the async seam with
`interactor_with_git_and_event_sink`, exactly as
`failed_launch_preparation_reaps_the_row_and_reports_spawn_failed.rs` does.

- `a_worktree_that_landed_off_its_planned_path_fails_the_launch.rs` — a
  `UseRemoteBranch("feature/x")` send is accepted while the fake reports no
  worktree for the branch (so the plan is the default path); by the time the
  launch task looks again the fake reports the branch checked out at another
  path. Expect: `SpawnFailed { reason: Some(..) }` naming both paths, the row
  deleted, `FakeTmux.created` empty, no launching/pending entry left. The
  fake's `checked_out_branches` is a plain map and the launch task is a
  `tokio::spawn`, so **do not** rely on mutating the map between the accept
  returning and the task running — that is scheduler order, not a guarantee.
  Give `FakeGitWorktree` a deterministic way to answer the plan's lookup and
  the build's lookup differently (e.g. a scripted sequence of answers for one
  branch, consumed in call order), documented on the fake.
- `a_launch_preparation_that_outruns_its_deadline_reports_spawn_failed.rs` —
  build the interactor with `.with_launch_config(LaunchConfig {
  launch_prep_deadline: <tens of ms>, ..Default::default() })` (it is the
  public builder `delta-bootstrap` already uses), hold the worktree build on a
  closed `WorktreeGate`, accept a worktree send, `await_launch()`. Expect:
  `SpawnFailed` whose `reason` is the `LaunchPreparationTimedOut` text, the row
  deleted, nothing launched, no launching entry left. The gate stays closed
  for the whole test — the point is that the timeout wins.

Run the gate for the backend (`make check` covers both parts) and fix whatever
it reports; the frontend is untouched by this task, so no frontend or e2e
changes are expected.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A `UseRemoteBranch` launch whose worktree lands on a path other than the
      one planned at accept time fails with a `spawn_failed` whose `reason`
      names the planned and actual paths, deletes the row, and launches
      nothing — pinned by
      `a_worktree_that_landed_off_its_planned_path_fails_the_launch.rs` (gate
      appended).
- [x] The preparation deadline is a `LaunchConfig` field
      (`launch_prep_deadline`, gate appended) with the 10-minute production
      default, overridable through `DELTA_LAUNCH_PREP_DEADLINE_MS` (gate
      appended), and a preparation that outruns it reports `spawn_failed`
      with the timeout as its reason — pinned by
      `a_launch_preparation_that_outruns_its_deadline_reports_spawn_failed.rs`
      (gate appended).
- [x] The docs that described the mismatch as a warning and the deadline as a
      fixed constant now describe the failure and the override.
- [x] `make check` is green.

## Out of scope

- Splitting the adapter-backed (Codex) spawn into accept and launch phases;
  it still builds its worktree inside the request and has no planned path to
  diverge from.
- Accepting a send to a spawning session as `queued` (still `409
  session_spawning`).
- Any change to how a healthy `UseRemoteBranch` launch reuses an existing
  worktree.
