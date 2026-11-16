---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: null
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check"
assignee: null
branch: task/1116-1657-stop-reusing-a-vanished-worktree-for-a-pr-session
created_at: 2026-11-16T07:57:13Z
updated_at: 2026-11-16T08:29:21Z
---

# fix(backend): stop launching a PR session in a worktree whose directory is gone

## Overview

A session started from a pull request arrives as
`WorktreeStartPoint::UseRemoteBranch(<branch>)`. Both halves of the worktree
work — `plan_worktree_launch_dir` and `resolve_worktree_launch_dir` in
`backend/crates/domain/delta-usecase/src/interactor/lifecycle/worktree_launch_dir.rs`
— reuse the worktree that already holds the branch, as reported by
`GitWorktree::worktree_path_for_branch`
(`backend/crates/gateway/git-worktree/src/git/mod.rs`, `impl` around line 332).
That method reads `git worktree list --porcelain` and returns the first path
whose `branch refs/heads/<branch>` line matches. It does not look at the
`prunable` line git emits for a worktree whose directory no longer exists.

Observed in dogfooding: another tool had checked the branch out in a worktree
under `/tmp`. The directory was later removed, but git still registered it
(`prunable gitdir file points to non-existent location`). Every new session
started from that PR was given that vanished path as its `cwd`, the launch
went through without any error, and `tmux new-session -c <missing dir>`
silently started the pane in `$HOME` (tmux 3.7c falls back to the home
directory when the start directory does not exist). The user got a Claude
session running in their home directory instead of on the PR's branch.

Fix both layers:

1. **Do not reuse a prunable worktree.** `worktree_path_for_branch` must treat
   an entry carrying a `prunable` line as absent, so planning falls back to
   the default path. Because git still considers the branch checked out in
   the missing worktree, `git worktree add` for that branch fails until the
   stale registration is gone, so `add_worktree_checkout` must run
   `git worktree prune` before `git worktree add`. `prune` removes only
   registrations whose directory is missing (and skips locked ones), so it
   does not disturb live worktrees. If git still refuses (e.g. the stale entry
   is locked), the launch fails with git's error rather than falling back to
   another directory. `launch_trust.rs` also calls
   `worktree_path_for_branch`; it must see the same answer, so a prunable
   entry no longer short-circuits the trust check there either.
2. **Never launch a pane in a directory that does not exist.** Before the
   pane is created on the launch task (`prepare_pane_launch` →
   `TmuxDriver::create_session` in
   `backend/crates/domain/delta-usecase/src/interactor/lifecycle/launch_prep.rs`),
   confirm the effective workdir exists and is a directory; if it does not,
   fail the launch so the browser gets a `spawn_failed` naming the missing
   directory, instead of tmux starting the pane in `$HOME`. This guards every
   launch path (fresh, PR, resume), not only the PR one. Use the existing
   `Workspace` port (or extend it) rather than calling `std::fs` from the
   use-case layer, and add an `Error` variant with a message that names the
   path.

Session states: this changes how a session in the **spawning / launching**
state finishes its preparation. The other states (closed, open + idle,
open + mid-turn, resuming, spawning / pending) are out of scope: they already
have a pane and do not pass through worktree resolution or pane creation.

Out of scope: whether delta should reuse a live worktree that sits outside
`DELTA_WORKTREE_BASE` (e.g. in another tool's scratch directory). This task
keeps that behaviour for worktrees that still exist.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A gateway test builds a real repository, checks a branch out in a
      secondary worktree, deletes that worktree's directory, and asserts
      `worktree_path_for_branch` returns `None` for that branch.
- [x] A gateway test with the same setup asserts `add_worktree_checkout` for
      that branch succeeds at a new path (the stale registration is pruned
      first) and the new path is on the branch.
- [x] Existing gateway tests that `worktree_path_for_branch` finds a live
      secondary worktree and the main working tree still pass.
- [x] A use-case test asserts that a launch whose effective workdir does not
      exist does not call `TmuxDriver::create_session`, and the session ends
      `failed` with a reason naming the missing directory.
- [x] A use-case test asserts that a launch whose workdir exists still creates
      the pane in that directory (the guard does not block the happy path).

### Before merge (verified outside the check command)

- [x] With a locally built delta against a scratch repository: register a
      worktree for a branch, delete its directory, start a session from that
      branch with the "use remote branch" start point, and confirm the pane's
      `pwd` is a new worktree under the worktree base on that branch, not
      `$HOME`.
