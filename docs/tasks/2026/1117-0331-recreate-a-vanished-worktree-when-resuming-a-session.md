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
branch: task/1117-0331-recreate-a-vanished-worktree-when-resuming-a-session
created_at: 2026-11-16T18:31:18Z
updated_at: 2026-11-16T19:07:17Z
---

# feat(backend): recreate a vanished worktree when resuming a closed session

## Overview

Resuming a closed session (`POST /api/sessions/{id}/open`, `open_session` in
`backend/crates/domain/delta-usecase/src/interactor/lifecycle/open_session.rs`)
now refuses when the session's `cwd` no longer exists:
`ensure_launch_dir_exists` (in `lifecycle/launch_prep.rs`) returns
`Error::LaunchDirMissing`, which `api_error.rs` maps to a plain HTTP 500 with no
wire code. That guard keeps tmux from silently starting the pane in `$HOME`,
but it leaves the session impossible to resume when the directory was a git
worktree that was removed (by delta's own storage cleanup, by another tool, or
by hand) while the branch it was on still exists.

Change the resume path so that, when the `cwd` is missing, delta rebuilds the
worktree and resumes into it:

1. **Identify the branch.** The session row carries `repo_root` and
   `branch_at_launch` (`backend/crates/domain/delta-model/src/session.rs`).
   The worktree can be rebuilt only when both are present and `repo_root`
   is still an existing git repository. Whether delta created the `cwd` or not
   does not matter: any session whose two fields identify a branch is rebuilt.
2. **Rebuild at the same path.** Check the branch out in a new worktree **at the
   session's original `cwd`**, not at a fresh path. `claude --resume` finds the
   transcript and the folder trust (`projects.<abs-path>` in `~/.claude.json`,
   see `backend/crates/gateway/git-worktree/src/trust.rs`) by the absolute
   launch directory, so resuming from another path would lose both. Reuse the
   gateway's existing checkout (`GitWorktree::add_worktree_checkout`, which
   already prunes stale registrations first and checks out an existing local
   branch or creates a tracking branch from `origin`) the way a launch from a
   pull request does (`worktree_launch_dir.rs`), rather than writing a second
   checkout routine. Create the missing parent directories of the `cwd` as the
   new-launch path does.
3. **Then resume** exactly as today: the existing `ensure_launch_dir_exists`
   check runs after the rebuild and must pass, and the rest of `open_session`
   (token, settings, pane spawn with `--resume`) is unchanged.
4. **When the branch cannot be identified** — `repo_root` or
   `branch_at_launch` is absent, `repo_root` no longer exists or is not a git
   repository, or the branch exists neither locally nor on `origin` — refuse
   before creating anything, with a new error that maps to a dedicated stable
   wire `code` in the response body (add it beside the other codes at the top
   of `backend/crates/apps/delta-server/src/api/api_error.rs`, e.g.
   `worktree_unrecoverable`), and a message that names the directory and why
   it could not be rebuilt. Document the code in
   `docs/guides/api/sessions.md` where `POST /api/sessions/{id}/open` is
   described. The frontend keeps showing the server's message as it does for
   other open failures; no new UI is added.
5. **When git refuses the checkout** (for example the branch is checked out in
   another worktree that still exists, so git will not check it out twice), the
   resume fails with git's error and the `cwd` is left absent — no fallback to
   another directory and no pane is created. Log it at `warn` with the session
   id, the path and the branch.

Log at `info` when a worktree is rebuilt (session id, path, branch).

Session states (per the product's session-state matrix): only **closed**
sessions reach this path. **open + idle / open + mid-turn / resuming** return
before the resume (`state.handle()` is set, or the remembered pane is adopted),
and **spawning** sessions have no transcript and are refused by the resume gate
as today; this task does not change those paths. Terminal-less
(adapter-backed) providers also return earlier and are out of scope.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A use-case test: a closed session whose `cwd` is missing, with
      `repo_root` and `branch_at_launch` set and the branch present, gets a
      worktree checked out at the original `cwd` on that branch, and a pane is
      spawned with the resume command in that directory.
- [x] Use-case tests for each unidentifiable case — `branch_at_launch` absent,
      `repo_root` absent, `repo_root` no longer an existing repository — return
      the new error, create no worktree and no pane, and leave the session
      closed.
- [x] A use-case test: when the checkout fails (the fake git gateway returns
      an error), the resume fails with that error, no pane is created and the
      session stays closed.
- [x] A use-case test: a closed session whose `cwd` exists resumes without
      touching git (no checkout is requested), as before.
- [x] A gateway test on a real repository: after a worktree directory holding
      a branch is deleted, checking the branch out again at the **same** path
      succeeds and the directory is on that branch.
- [x] A server test asserts the new error is answered with its dedicated wire
      `code` in the response body.

### Before merge (verified outside the check command)

- [x] With a debug `delta-server` built from the branch (isolated `HOME`,
      data directory, worktree base, tmux socket and port, and
      `DELTA_CLAUDE_BIN` pointing at a stub that records its working directory
      and arguments), a session launched from a pull request is closed, its
      worktree directory is deleted with `rm -rf`, and
      `POST /api/sessions/{id}/open` recreates the directory at the same path
      on the session's branch; the pane process's working directory is that
      path and the stub received `--resume <session id>`.
