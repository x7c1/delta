---
status: completed
pipeline_phase: null
plan: null
base_ref: null
blocked_by: []
subagent_type: general-purpose
retries_remaining: 1
check_command: "cd backend && cargo build && cargo test && cargo clippy --all-targets -- -D warnings"
assignee: null
branch: task/0626-0532-feat-worktree-dir-naming-by-repo
created_at: 2026-06-26T05:32:51Z
updated_at: 2026-06-30T04:39:30Z
---

# feat(worktree): name per-session worktree dirs by repository identity

## Overview

`$DELTA_WORKTREE_BASE` (default `~/.delta/worktrees/`) currently holds per-session
worktree directories named `delta-<session-id>`. Listing the base directory
shows nothing but a wall of UUID-suffixed names with no hint of which repository
each worktree belongs to, which makes hand-cleanup, `cd`-into, and add-to-VS-Code-
workspace flows unnecessarily painful when several repositories are dogfooded in
parallel.

Change the on-disk worktree directory name to embed the repository identity:

- New format: `<org>-<repo>-<session-id>` (e.g. `x7c1-delta-019ef2a9-bb6d-74c2-9b00-307d43f03b84`).
- Origin-less local-only repos fall back to `<repo>-<session-id>` (the existing
  `display_name` helper already returns the working-tree basename in that case).
- The git **branch** name created for new-branch start points must stay
  `delta-<session-id>` — the frontend's `displayBranch()` regex
  (`^delta-([0-9a-f]{8})-...$`) matches the branch and would no longer shorten
  the navigator line if the branch were renamed. Worktree path and branch name
  diverging is fine; git does not require them to match.

The repository identity is derived using the same `identity_key` / `display_name`
helpers `spawn_fresh` already uses today to populate
`Session.repository_display_name`. Today that derivation runs *after* the
worktree path is built; this task hoists the derivation so the worktree path
can consume it, and ensures `origin_url` is called only once.

### What changes

1. **Hoist `repository_display_name` derivation** in
   `backend/crates/domain/delta-usecase/src/interactor/lifecycle/spawn_fresh.rs`
   so it runs *before* the worktree-path construction inside the
   `match worktree { Some(spec) => ... }` block. Reuse the same binding at the
   later `insert_spawning_session` call site so `origin_url` is invoked only
   once per spawn.

2. **Add a slug helper** in
   `backend/crates/domain/delta-usecase/src/repository.rs`:

   ```rust
   /// Slugify a [`display_name`] result for use as a filesystem path segment.
   ///
   /// Replaces `/` with `-` (so `org/repo` becomes `org-repo`) and replaces any
   /// character outside `[A-Za-z0-9._-]` with `_`. Returns the input unchanged
   /// when it already consists only of safe characters.
   pub fn worktree_dir_slug(display_name: &str) -> String { ... }
   ```

   Re-export it from `lib.rs` next to `display_name` / `identity_key`. Cover
   it with unit tests: `"x7c1/delta"` → `"x7c1-delta"`, `"delta"` → `"delta"`,
   `"org/sub/repo"` → `"org-sub-repo"`, a string containing spaces / `!` →
   the sanitized form, `""` → `""`.

3. **Build the new worktree path**:

   ```rust
   let slug = repository_display_name
       .as_deref()
       .map(worktree_dir_slug)
       .filter(|s| !s.is_empty())
       .unwrap_or_else(|| "delta".to_owned());
   let default_path =
       format!("{}/{}-{}", self.worktree_base, slug, session_id.as_str());
   ```

   Leave `let branch = format!("delta-{}", session_id.as_str());` unchanged.

4. **Update the three existing worktree-path tests** to expect the new shape:

   - `backend/crates/domain/delta-usecase/src/interactor/lifecycle/tests/new_session_with_worktree_seeds_trust_for_the_worktree_path.rs`
   - `backend/crates/domain/delta-usecase/src/interactor/lifecycle/tests/new_session_with_use_branch_checks_out_a_new_worktree.rs`
   - `backend/crates/domain/delta-usecase/src/interactor/lifecycle/tests/new_session_with_worktree_launches_in_the_worktree.rs`

   Configure each fake to drive a deterministic slug (either set `origin_url`
   to a known value or rely on the basename fallback) and assert the resulting
   path explicitly.

5. **Add a new dir-naming unit test** in
   `backend/crates/domain/delta-usecase/src/interactor/lifecycle/tests/`
   that pins the new behaviour independently: fake `origin_url` returns
   `Some("https://github.com/x7c1/delta")` → worktree path is
   `<TEST_WORKTREE_BASE>/x7c1-delta-<session-id>` and branch is
   `delta-<session-id>` (unchanged).

6. **Update doc comments** describing the legacy `delta-<id>` worktree path
   shape:

   - `backend/crates/libs/delta-bootstrap/src/lib.rs:61`
   - `backend/crates/domain/delta-model/src/session.rs:42`
   - `backend/crates/domain/delta-usecase/src/interactor/mod.rs:91`
   - `backend/crates/domain/delta-usecase/src/ports/git_worktree.rs` lines
     8, 11, 15, 21, 24, 28, 171 (the **branch** mentions stay correct — only
     the **path** descriptions need updating)

7. Verify by grep that `self.git_worktree.origin_url(` is called at most once
   per spawn path in `spawn_fresh.rs`.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `worktree_dir_slug` exists in `delta-usecase` / `repository.rs`, is
      `pub` re-exported from `lib.rs`, and its unit tests cover the cases
      listed above.
- [x] A new lifecycle test asserts that the worktree path passed to
      `git_worktree.create_worktree` for a session whose fake `origin_url`
      returns `Some("https://github.com/x7c1/delta")` is
      `<TEST_WORKTREE_BASE>/x7c1-delta-<session-id>`, AND that the branch
      argument is `delta-<session-id>` (unchanged shape).
- [x] The three existing worktree-path tests are updated and pass.
- [x] `self.git_worktree.origin_url(` is called at most once per spawn in
      `spawn_fresh.rs` (verified by grep during code review).
- [x] `cd backend && cargo build && cargo test && cargo clippy --all-targets -- -D warnings` passes.

### Manual / on-hardware (verified by a human before merge)

- [ ] After `make dev` (or `make mock`), spawn a worktree-enabled session,
      and observe that `ls ~/.delta/worktrees/` includes a directory named
      `<org>-<repo>-<session-id>` for the new spawn. Existing `delta-<uuid>`
      entries from older spawns are unaffected.

## Out of scope

- Renaming or migrating existing on-disk worktrees.
- Changing the git branch name.
- Cleanup-on-close behaviour for worktrees.
- Frontend display changes (the cwd tooltip already reads the path verbatim).
