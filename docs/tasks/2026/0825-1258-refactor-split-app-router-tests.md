---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && A=backend/crates/apps/delta-server/src/app && [ -f $A/mod.rs ] && [ -f $A/tests/mod.rs ] && [ ! -f backend/crates/apps/delta-server/src/app.rs ] && ! grep -q "mod tests {" $A/mod.rs && [ "$(wc -l < $A/mod.rs)" -lt 120 ] && [ "$(ls $A/tests/*.rs | grep -vc mod.rs)" -ge 6 ] && [ "$(cd backend && cargo test -p delta-server --lib -- --list 2>/dev/null | grep -c "^app::tests::")" -eq 50 ]'
assignee: null
branch: task/0825-1258-refactor-split-app-router-tests
created_at: 2026-08-25T12:58:42Z
updated_at: 2026-08-25T13:41:53Z
---

# refactor(server): move the router tests out of app.rs into a tests/ directory module

## Overview

`backend/crates/apps/delta-server/src/app.rs` is 2066 lines, and only the first
84 are production code: the `router()` composition root (a list of
`RouteBinder::bind()` calls) and the `health` handler. The remaining 1982 lines
are one inline `#[cfg(test)] mod tests` holding 50 HTTP-level tests against the
assembled router plus their shared fixtures (`test_state`, `register_session`,
`error_code`, `list_launch_options`, `register_clone_root`,
`test_state_with_unavailable_gh`, `test_state_with_gh_stub`,
`test_state_with_only_claude_present`). Every endpoint added to the router adds
tests here and never adds production code, so the file grows unbounded in the
wrong dimension: a reader who wants to see what the server binds has to open a
2000-line file, and a reader who wants the launch-option tests has to find them
among 49 unrelated ones.

Split the test module into a directory module, keeping the production code
untouched in behaviour. This is a pure move; no test is rewritten, renamed,
added or dropped.

**Target layout** (the repository's module convention is `mod.rs` — 41
directory modules use it and none use the `foo.rs` + `foo/` pairing, so
`app.rs` becomes `app/mod.rs`):

```
backend/crates/apps/delta-server/src/app/
├── mod.rs              # the 84 production lines, then `#[cfg(test)] mod tests;`
└── tests/
    ├── mod.rs          # `mod <subject>;` list + fixtures shared by ≥2 subjects
    ├── <subject>.rs    # one file per endpoint group, see below
    └── ...
```

**Group by endpoint subject**, the way `delta-sqlite/src/store/tests/`
(`clone_roots.rs`, `launch_options.rs`, `prompt_templates.rs`, ...) already
does in this workspace. The other precedent, one file per test under
`delta-usecase/src/interactor/lifecycle/tests/`, does not fit here: these 50
tests cluster tightly by endpoint, and a fixture like `test_state_with_gh_stub`
is only meaningful to the four `prs_*` tests that share it. Suggested
partition of the 50 tests (adjust the names if a better cut appears while
moving, but keep every file a coherent endpoint group):

| file | tests |
| --- | --- |
| `sessions.rs` | `health_returns_ok`, `get_version_returns_a_version_string_shaped_like_v_prefixed`, `list_sessions_rejects_a_malformed_cursor`, `comms_requires_a_session_id`, `release_send_replies_conflict_with_the_stable_code_when_not_releasable` |
| `hooks.rs` | `user_prompt_submit_hook_registers_and_responds`, `pre_tool_use_hook_returns_ok`, `session_start_hook_returns_ok`, `session_end_hook_returns_ok` |
| `permissions.rs` | `permission_request_hook_passes_through_on_timeout`, `permission_decision_resolves_the_blocked_hook`, `the_claude_hook_envelope_is_unchanged_for_allow_and_deny`, `a_session_scoped_decision_is_refused_for_a_provider_without_the_capability`, `permission_decision_for_an_unknown_request_is_a_conflict` |
| `status_line.rs` | the four `status_line_*` tests |
| `workdir.rs` | `workdir_list_*` (2), `workdir_recent_returns_an_empty_list_when_no_sessions`, `workdir_git_rejects_a_blank_path`, `workdir_git_branches_rejects_a_blank_path`, `open_cwd_*` (3) |
| `pull_requests.rs` | the four `prs_*` tests + fixtures `test_state_with_unavailable_gh`, `test_state_with_gh_stub` |
| `providers.rs` | `providers_reports_availability_per_provider` + fixture `test_state_with_only_claude_present` |
| `clone_roots.rs` | `repositories_returns_an_empty_list_when_no_sessions`, `clone_roots_round_trip_create_list_delete`, `clone_repository_*` (3), `create_clone_root_*` (4), `delete_unknown_clone_root_is_idempotent` + fixture `register_clone_root` |
| `launch_options.rs` | the five launch-option tests + fixture `list_launch_options` |
| `prompt_templates.rs` | the four prompt-template tests |

Fixtures used by two or more subject files (`test_state`, `register_session`,
`error_code`) live in `tests/mod.rs`; a fixture used by a single subject lives
in that subject's file. Give each subject file a one-line `//!` doc comment
naming the endpoint group, as the store precedent does. Keep the imports
minimal per file (`use super::*;` from `tests/mod.rs` re-exporting what the
subjects need is fine; do not leave unused imports — clippy runs with
`-D warnings`).

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `backend/crates/apps/delta-server/src/app.rs` no longer exists;
      `backend/crates/apps/delta-server/src/app/mod.rs` holds the production
      code and declares `#[cfg(test)] mod tests;` with no inline test module
      (`grep "mod tests {"` finds nothing there), and stays under 120 lines
      (gate appended to `check_command`).
- [x] `backend/crates/apps/delta-server/src/app/tests/mod.rs` exists and at
      least 6 subject files sit beside it (gate appended to `check_command`).
- [x] The test count is unchanged: `cargo test -p delta-server --lib -- --list`
      lists exactly 50 tests under `app::tests::` — the same 50 that exist on
      `main` today (gate appended to `check_command`; the paths gain a
      subject segment, e.g. `app::tests::launch_options::create_then_list_and_delete_launch_option`).
- [x] `make check` is green: `cargo fmt --check`, build, all backend tests,
      clippy with `-D warnings` (so no unused import survives the move), the
      generated-bindings freshness check, and the frontend/e2e stages that the
      canonical gate runs regardless.

### Manual / on-hardware (verified by a human before merge)

- [ ] Reading the diff confirms it is a pure move: each test body is byte-for-byte
      what it was in `app.rs` apart from indentation and the import lines at the
      top of each file. If refine deliberately rewrote something (e.g. a doc
      comment that referred to a location that no longer resolves), the PR body
      names it.

## Out of scope

- Any change to `router()`, `RouteBinder`, or the handlers — the production
  code moves from `app.rs` to `app/mod.rs` verbatim.
- Adding, dropping, renaming or rewriting tests. Coverage gaps noticed while
  moving belong in a separate task.
- Splitting other large files in `delta-server` (`api/`, `hooks/`); this task
  is only `app.rs`.
