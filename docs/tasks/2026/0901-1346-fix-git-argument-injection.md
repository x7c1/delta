---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && grep -rq "rejects_a_ref_name_beginning_with_a_dash" backend/crates && grep -rq "rejects_an_owner_beginning_with_a_dash" backend/crates'
assignee: null
branch: task/0901-1346-fix-git-argument-injection
created_at: 2026-09-01T13:46:00Z
updated_at: 2026-09-01T14:12:00Z
---

# fix(git): reject ref and repo names that git could parse as flags

## Overview

The git gateway passes branch/ref names and repo owner/name to `git` and `gh`
as positional arguments with no `--` separator and no rejection of names that
begin with `-`, so a name like `--upload-pack=/tmp/evil` reaching a fetch would
be parsed by git as a flag, not a ref — argument injection. All subprocesses
are already spawned without a shell (a good property to preserve), but a
flag-shaped argument does not need a shell to do harm.

Where the untrusted names flow:
- **Branch/ref names**: `WorktreeStartPoint::RemoteBranch(String)` and
  `WorktreeStartPoint::UseRemoteBranch(String)`
  (`backend/crates/domain/delta-usecase/src/ports/git_worktree.rs`) carry a
  remote branch short name that comes from the new-session request. They reach
  `git` positionally in `backend/crates/gateway/git-worktree/src/git.rs`:
  `["fetch", REMOTE, name]` (~L280), `["worktree", "add", "-b", branch, …]`
  (~L304), `["fetch", REMOTE, branch]` (~L356),
  `["worktree", "add", worktree_path, branch]` (~L381),
  `["worktree", "add", "--track", "-b", branch, …]` (~L392). **None are
  validated** in the domain layer.
- **Repo owner/name**: validated by `check_path_segment`
  (`.../interactor/repository/clone_repository/mod.rs:308`), which rejects
  empty / `.` / `..` / `/` / `\` / NUL — **but not a leading `-`**. The slug
  `format!("{owner}/{name}")` is passed to `gh repo clone` positionally
  (`backend/crates/gateway/gh-cli/src/gh.rs` ~L138) with no `--`, so a `-`-led
  owner is a `gh` flag.

### The fix (two layers)

1. **Domain-layer validation (primary gate — the acceptance test targets
   this).** Add a `check_ref_name` validator next to `check_path_segment`
   (reuse `Error::InvalidRepositoryRef` or add a sibling variant such as
   `Error::InvalidBranchName` — your call, mirror the existing error-type
   style). It rejects a ref/branch name that: is empty, **begins with `-`**,
   contains whitespace or ASCII control chars, or contains NUL. Keep it
   minimal and well-commented (the point is the leading-`-` flag defense, not a
   full `git check-ref-format`). Apply it to the branch name carried by
   `WorktreeStartPoint::RemoteBranch` / `UseRemoteBranch` at the point the
   start point is **constructed from request input** (find the construction
   site; if there is one narrow seam, validate there — otherwise validate at
   the worktree-planning entry `worktree_launch_dir.rs`'s `plan`, which every
   worktree start point funnels through, so the name is rejected **before**
   `git.rs` spawns anything). Also **extend `check_path_segment` to reject a
   leading `-`** for owner/name (the existing checks stay).
2. **Gateway-layer `--` separators (defense in depth).** Where the underlying
   command accepts a `--` end-of-options separator, add it so a name can never
   be parsed as a flag even if validation is bypassed:
   - `gh repo clone -- <slug> <dest>` in `gh.rs` (gh supports `--`).
   - In `git.rs`, add `--` where git accepts it for the argument in question.
     Note: not every git subcommand takes `--` for a *refspec* (e.g. plain
     `git fetch <remote> <refspec>` does not), so where `--` is not valid for
     that position, the domain-layer leading-`-` rejection is the guarantee —
     do not add a `--` that changes git's parsing of a refspec/branch and
     breaks the command. Verify each command you touch still works
     (`fetch_remote_branches_lists_origin_branches_offline` and the other
     git.rs tests must stay green). The comment at `gh.rs` L133-137 currently
     reasons only about `destination`; update it to cover the slug too.

**Preserve the existing good properties — do not regress them:** no shell
invocation (keep `Command::new(...).args(...)`, never `sh -c`), the path
canonicalization in `workspace-fs`, and the open-cwd allowlist
(`interactor/open_cwd`). This task only adds validation and `--` separators.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A branch/ref name beginning with `-` (e.g. `--upload-pack=/tmp/x`) is
      rejected by the domain-layer validator **before** any `git` subprocess is
      spawned — a unit test drives the validation (or the worktree-planning
      entry) with such a name and asserts the error, using no real git (test
      name contains `rejects_a_ref_name_beginning_with_a_dash`; grepped by
      `check_command`). A normal name like `main` / `feature/x` still passes.
- [x] An owner (or repo name) beginning with `-` is rejected by
      `check_path_segment` (test name contains
      `rejects_an_owner_beginning_with_a_dash`; grepped by `check_command`), and
      the pre-existing rejections (empty / `.` / `..` / `/` / `\` / NUL) and
      valid names still behave as before.
- [x] `gh repo clone` is invoked with a `--` separator before the slug
      (inspect `gh.rs`), and any `--` added in `git.rs` leaves the existing
      git.rs tests green.
- [x] `make check` passes green — no shell invocation was introduced, and the
      worktree / clone / PR-search paths still work.

### Manual / on-hardware (verified by a human before merge)

- [ ] A real new-session-on-a-remote-branch worktree launch still works end to
      end (normal branch names are unaffected by the validation). (Non-blocking
      for merge under the agreed CI-green autonomous policy; recorded for
      dogfooding.)

## Out of scope

- A full `git check-ref-format` implementation — reject the flag-shaped and
  obviously-invalid cases; do not reproduce git's entire ref grammar.
- Validation of paths already covered by `workspace-fs` canonicalization and
  the open-cwd allowlist (keep them; do not duplicate).
- The `gh api graphql` search call (`search_prs`) — its values are
  `key=value` strings that cannot lead with `-`, so they are already safe.
