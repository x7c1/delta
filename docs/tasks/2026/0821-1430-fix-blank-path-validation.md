---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check'
assignee: null
branch: task/0821-1430-fix-blank-path-validation
created_at: 2026-08-21T14:30:41Z
updated_at: 2026-08-22T00:38:00Z
---

# fix(api): reject blank paths the way the API docs already promise

## Overview

Two request handlers in `backend/crates/apps/delta-server/src/api/mod.rs`
promise to reject a blank `path` with a `400` and do not. They are the same
defect: a documented blank-rejection that the code does not implement. Neither
is reachable from the UI, so this is a contract fix rather than a user-visible
bug fix — but the contract is what other clients and the mock server are
written against, and it is currently wrong in two places.

### 1. `create_clone_root` turns a blank path into `/` and registers it (201)

`create_clone_root` (`api/mod.rs:322`) canonicalises before it validates:

```rust
let trimmed = req.path.trim();
let canonical = {
    let stripped = trimmed.trim_end_matches('/');
    if stripped.is_empty() { "/" } else { stripped }
};
if canonical.is_empty() {
    return Err(ApiError::BadRequest(
        "a clone root must have a non-blank `path`".to_owned(),
    ));
}
```

`canonical` can never be empty — the `else` branch is non-empty by
construction and the `if` branch yields `"/"` — so the blank check below it is
dead code that no input reaches. `""`, `"   "` and `"///"` all become `/` and
register with `201`.

Four other places state the opposite: `docs/guides/api/workdirs.md:308`
(“**400** — a blank or relative `path`”), the wire field doc
(`delta-wire/src/rest/clone_root_create_request.rs:18`), this handler's own doc
comment (“must be a non-blank absolute path”), and the mock server
(`frontend/packages/testing/api-mocks/src/handlers.ts:819-825`, which returns
`400`). The implementation is the outlier; the mock, which advertises itself as
reproducing the real server's contract, is currently the correct one.

Registering `/` is not harmless even though the UI cannot trigger it: every
`GET /api/repositories` calls `scan_one_root` on each registered root, which
`read_dir`s the root and `canonicalize`s each child, so a `/` row makes every
repository listing walk `/usr`, `/var`, `/proc` and the rest.

**Preserve the documented behaviour for a literal `/`.** `{"path": "/"}` is
non-blank and absolute, so it must keep returning `201` — as it does in the
mock. Only *blank* input may become a `400`. Likewise `"/home/dev/projects/"`
must keep canonicalising to `/home/dev/projects` with `201`.

### 2. `WorkdirGitQuery::require_path` checks empty, not blank

```rust
/// The required `path`, or a `400` when it is missing or blank.
fn require_path(&self) -> Result<&str, ApiError> {
    match self.path.as_deref() {
        Some(path) if !path.is_empty() => Ok(path),
        _ => Err(ApiError::BadRequest(
            "a `path` query parameter is required".to_owned(),
        )),
    }
}
```

`" "` is not empty, so it passes and reaches the git gateway. The doc comment
one line above says “blank”, and so do `workdirs.md:93`
(`GET /api/workdir/git`) and `workdirs.md:117`
(`GET /api/workdir/git/branches`). Nothing downstream trims it: `git-worktree`
trims git's *output*, never the path it is handed.

Both sibling handlers in this same file already do the right thing —
`create_launch_option` (`api/mod.rs:508`) and `open_cwd` (`api/mod.rs:751`)
both `trim()` and then test `is_empty()`. `require_path` is the one that was
missed. Trim once and return the trimmed slice, so the two call sites
(`api/mod.rs:461` and `:476`) receive a path with no surrounding whitespace.

### Scope note

The prose in `docs/guides/api/workdirs.md` is already correct for all three
endpoints, so this task is expected to change implementation and tests, not the
API guide. If you find a doc sentence that the corrected implementation
contradicts, fix it — but do not rewrite passages that were already right.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `POST /api/clone-roots` returns `400` for `{"path": ""}`, for a
      whitespace-only path, and for `{"path": "///"}`, each asserted by a test
      in `delta-server`'s `app.rs` test module alongside the existing
      `create_clone_root_rejects_a_non_absolute_path`.
- [x] `POST /api/clone-roots` still returns `201` for `{"path": "/"}` and still
      canonicalises `{"path": "/home/dev/projects/"}` to `/home/dev/projects`,
      each asserted by a test — the blank fix must not narrow the documented
      contract.
- [x] No unreachable validation branch is left behind in `create_clone_root`:
      the blank check runs on input that can actually be blank, and
      `cargo clippy --all-targets -- -D warnings` stays clean.
- [x] `GET /api/workdir/git` and `GET /api/workdir/git/branches` each return
      `400` for a whitespace-only `path`, asserted by a test per endpoint
      (neither endpoint has a `require_path` test today).
- [x] A `path` with surrounding whitespace that is otherwise valid reaches the
      gateway trimmed, asserted by a test — `require_path` returns the trimmed
      slice rather than the raw one.
- [x] The mock server's clone-root handler and the real server agree on blank
      input (both `400`) and on `/` (both `201`); the mock's "reproduces the
      real server's contract" comment is true again.

### Manual / on-hardware (verified by a human before merge)

- [ ] Reading the diff confirms the change is confined to input validation:
      no endpoint gains or loses a status code beyond the blank cases named
      above, and no scan/clone behaviour is touched.

## Out of scope

- Whether registering `/` as a clone root *should* be allowed. It is currently
  documented and mocked as valid, so it stays valid here; restricting it is a
  product decision, not a contract fix.
- The `repo_owner` / `repo_name` single-path-component check
  (`workdirs.md:239`), the blank `name` check on `POST /api/launch-options`
  (`settings.md:150`), and the blank `path` check on `POST /api/open-cwd`
  (`workdirs.md:142`) — all three were verified as correctly implemented while
  scoping this task.
- Any other endpoint's validation. This task covers exactly the two handlers
  named above.
