---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && git grep -q 'remove_worktree' -- backend/crates/domain/delta-usecase/src/ports/git_worktree.rs && git grep -q 'prune_worktrees' -- backend/crates/domain/delta-usecase/src/ports/git_worktree.rs && git grep -q 'forget_dir_trusted' -- backend/crates/domain/delta-usecase/src/ports/git_worktree.rs && ! git grep -q 'Nothing on disk is deleted' -- docs/guides/api/sessions.md"
assignee: null
branch: task/1006-0750-feat-sessions-remove-a-deleted-sessions-worktree-and-branch-when-they-hold-no-work
created_at: 2026-10-05T22:46:51Z
updated_at: 2026-10-05T23:58:13Z
---

# feat(sessions): remove a deleted session's worktree and branch when they hold no work

## Overview

Removing a session (`DELETE /api/sessions/{id}`,
`backend/crates/domain/delta-usecase/src/interactor/lifecycle/delete_session.rs`)
deletes the row and its cascade and touches nothing on disk: the git
worktree Delta created for the session under `worktree_base`, its
`delta-<session id>` branch in the user's repository, and the trust entry
Delta seeded for the worktree path in `~/.claude.json`
(`backend/crates/gateway/git-worktree/src/trust.rs`) all stay. The API guide
says so (`docs/guides/api/sessions.md`, "Nothing on disk is deleted"). On a
machine that has used Delta for a while this leaves dozens of worktrees
nobody owns: a user who removes a session from the list has no reason to
expect its working copy to outlive it, and nothing in the app removes them
later.

The rule this task introduces: **a session owns the worktree and the branch
Delta created for it, and Delta never destroys the user's work.** Removing
a session removes those two when they hold no work; when they do, Delta
keeps them, says so, and leaves them for the user (a later change lists
such leftover worktrees in Settings → Storage with a per-item remove).

### Rule

For a closed session being removed (the existing open/spawning refusals
stay as they are, checked before anything is touched):

1. The session's `cwd` is a worktree Delta created **iff** it lies under
   `worktree_base` (`is_under_worktree_base`,
   `interactor/lifecycle/mod.rs`) and `repo_root` is set. A session that ran
   in the user's own repository or in a scratch directory under
   `sessions/` has nothing to remove here (the scratch directory under the
   data directory is not a repository; it stays, and the Storage cleanup
   will account for it later).
2. The worktree is removed only when it is clean: `git worktree remove
   <path>` without `--force` (git itself refuses a worktree with modified
   or untracked files). A refusal keeps the worktree.
3. The branch is deleted only when it is one Delta created — named
   `delta-<session id>` — and only with `git branch -d` (not `-D`), which
   git refuses for a branch not merged into its upstream or HEAD. A refusal
   keeps the branch. A session started from an existing branch (a pull
   request's, for example) never has its branch deleted.
4. After a successful removal, `git worktree prune` runs in `repo_root`,
   and the `~/.claude.json` trust entry for the worktree path is removed
   (a new `forget_dir_trusted` beside `ensure_dir_trusted`; a missing entry
   is not an error).
5. Whatever was kept is reported: the usecase returns what it removed and
   what it kept (worktree kept because dirty; branch kept because unmerged),
   the handler logs it at `info`, and the row is deleted regardless — the
   kept items become orphans for the Storage view, never a reason to refuse
   the removal. A git failure other than a refusal (the repository is gone,
   git is missing) is logged and treated as "kept" too; the removal of the
   session itself does not fail because of it.

### Change

- `backend/crates/domain/delta-usecase/src/ports/git_worktree.rs`: add
  `remove_worktree(repo_root, path) -> Result<WorktreeRemoval>` (removed /
  kept-dirty), `delete_branch_if_merged(repo_root, branch) -> Result<BranchDeletion>`
  (deleted / kept-unmerged / absent), `prune_worktrees(repo_root)`, and
  `forget_dir_trusted(dir)`. Give the result enums their own modules (one
  public type per module) and implement them in
  `backend/crates/gateway/git-worktree/src/git.rs` (distinguish git's
  refusal — non-zero exit with its known message — from other failures) and
  `trust.rs`; mirror them in
  `backend/crates/domain/delta-usecase/src/interactor/testing/fake_git_worktree.rs`
  with a programmable outcome per path.
- `delete_session.rs`: apply the rule above after the state checks and
  before (or after — pick one and justify it: the row must go even if git
  fails, and the worktree path is read from the row) the row deletion.
  Replace the "left in place" log line with one that states what was
  removed and what was kept.
- The DELETE handler (`backend/crates/apps/delta-server/src/api/mod.rs`,
  `delete_session`) keeps answering 204 with no body; the outcome is
  logged. Do not add response fields in this change.
- `docs/guides/api/sessions.md`, `DELETE /api/sessions/{id}`: replace the
  "Nothing on disk is deleted" paragraph with the rule (what is removed,
  what is kept and why, that the agent's own transcript files are never
  touched). Keep the rest of the section.
- `frontend/packages/apps/web/src/features/navigator/SessionNode.tsx`:
  the comment describing `Remove` (around line 122) says the worktree
  stays; update it to the rule. If the UI shows confirmation copy for
  Remove that promises the worktree stays, update that copy too; otherwise
  no UI change.

### Tests

- Usecase tests (`interactor/lifecycle/tests/`), one file each, against
  the fake git worktree and fake store: a clean worktree with a merged
  `delta-<id>` branch is removed, pruned, and its trust entry forgotten; a
  dirty worktree is kept and the branch left alone, and the row is still
  deleted; a merged worktree whose branch is not `delta-`-prefixed keeps
  its branch; a session whose `cwd` is outside `worktree_base` touches git
  not at all; a git failure keeps everything and still deletes the row.
- Gateway tests in `git.rs` against a temporary repository (the
  `init_repo_with_commit` helper): removing a clean worktree succeeds and
  `git worktree list` no longer shows it; a worktree with an untracked file
  is refused and kept; `git branch -d` on an unmerged branch is refused and
  reported as kept; `forget_dir_trusted` removes exactly one `projects`
  key and leaves the rest of `~/.claude.json` (use a temp config path, as
  the existing trust tests do).

## Acceptance criteria

### Automated (pipeline-verified)

- [x] The `GitWorktree` port has `remove_worktree`, `prune_worktrees` and
      `forget_dir_trusted` (`git grep` gates), and the API guide no longer
      claims nothing on disk is deleted (negative `git grep` gate).
- [x] Usecase tests cover: clean + merged → removed, pruned, trust
      forgotten; dirty → kept, row still deleted; non-`delta-` branch →
      branch kept; `cwd` outside `worktree_base` → git untouched; git
      failure → kept, row deleted.
- [x] Gateway tests cover worktree removal, the dirty refusal, the
      unmerged-branch refusal and trust-entry removal against a temporary
      repository.
- [x] `make check` passes.

### Before merge (verified outside the check command)

- [ ] On the development machine, remove a closed session that ran in a
      Delta-created worktree with no uncommitted changes: the worktree
      directory is gone, `git worktree list` in the repository no longer
      shows it, the `delta-<id>` branch is gone, and `~/.claude.json` has
      no `projects` key for the path. Then remove one whose worktree has an
      uncommitted file: the directory and branch remain and the server log
      says they were kept.
