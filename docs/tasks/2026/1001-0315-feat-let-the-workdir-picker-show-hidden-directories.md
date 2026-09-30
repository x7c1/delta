---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, user-experience, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && grep -rq 'include_hidden' backend/crates/gateway/workspace-fs/src/ && grep -rq 'include_hidden' backend/crates/apps/delta-server/src/ && ! grep -rq 'dot segment the picker cannot enter' scripts/"
assignee: null
branch: task/1001-0315-feat-let-the-workdir-picker-show-hidden-directories
created_at: 2026-09-30T18:10:30Z
updated_at: 2026-09-30T18:33:48Z
---

# feat(web): let the workdir picker show hidden directories

## Overview

The new-session working-directory picker cannot reach any directory that sits
below a dot-directory. `FsWorkspace::list` in
`backend/crates/gateway/workspace-fs/src/workspace/browse.rs` skips every entry
whose name starts with `.`, and `WorkdirPickerBody.tsx`
(`frontend/packages/apps/web/src/features/composer/`) has no path input — the
only ways in are the Recent list and the Browse list, which starts at `$HOME`
and steps one directory at a time. So a clone under `~/.local/share/...` or a
linked worktree under `<repo>/.tmp/worktrees/...` can only be picked after it
has already been used once (and shows up in Recent).

The same limitation breaks the real-claude browser smoke. The smoke's helper
`navigateBrowseTo` in `frontend/packages/apps/web/e2e-fake/support/app.ts`
clicks down from `$HOME` segment by segment, so `scripts/e2e-real-claude.sh`
refuses to run (`die "smoke workdir path contains a dot segment the picker
cannot enter"`) whenever the main checkout lives under a dot-directory — for
example a clone under `~/.local/share/`.

Add an opt-in "show hidden directories" toggle to the Browse section:

1. **Backend.** Thread an `include_hidden: bool` flag from
   `GET /api/workdir/list` (a new optional query parameter on
   `WorkdirListQuery` in `backend/crates/apps/delta-server/src/api/mod.rs`,
   e.g. `?hidden=true`; absent means `false`) through
   `InteractorCore::browse_workdir` and the `Workspace::list_dirs` port down
   to `FsWorkspace::list`. With the flag off the listing is exactly what it is
   today (dot-directories hidden); with it on, dot-directories are listed
   too, still directories only and still sorted case-insensitively. Update
   the doc comments that currently say "dot-directories hidden" /
   "dot-directories excluded" (`api/mod.rs`, `browse_workdir.rs`,
   `ports/dir_listing.rs`, `ports/workspace.rs`) so they describe the default
   and the flag. Update every `Workspace` implementation and fake so the
   workspace compiles.
2. **API client.** `ApiClient.getWorkdirList` (`frontend/packages/gateway/api-client/src/http.ts`)
   and `useWorkdirListQuery` (`query-hooks.ts`) take the flag, and the query
   key (`queryKeys.workdirList` in `query-keys.ts`) includes it so the two
   listings are cached separately. `useHomeDirQuery`, which shares the
   `workdirList(null)` key for path abbreviation, keeps working unchanged.
   Update any MSW / api-mocks handler for the endpoint so it honours the
   parameter.
3. **Picker UI.** Add a small toggle (a checkbox labelled "Show hidden") to
   the Browse section header of `WorkdirPickerBody.tsx`. It defaults off each
   time the picker opens, and flipping it re-lists the current directory
   without changing the current browse path or the candidate. Hidden entries
   render like any other directory row.
4. **Browser smoke.** `navigateBrowseTo` turns the toggle on before clicking a
   segment that starts with `.` (turn it on once, up front, when any segment
   of the target path starts with `.`), and its doc comment drops the "no
   dot-segments" restriction. In `scripts/e2e-real-claude.sh`, remove the
   dot-segment `case` guard and rewrite the comment above `MAIN_REPO_GIT_DIR`
   accordingly; keep the `$HOME` guard, since the picker still starts at
   `$HOME` and has no path input. Also update the matching sentence in
   `startNewSession`'s doc comment if it mentions dot segments.

Out of scope: a free-text path input, and persisting the toggle across picker
openings.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A `workspace-fs` unit test lists a directory containing a dot-directory,
      a normal directory and a file: with `include_hidden = false` only the
      normal directory is returned, with `include_hidden = true` both
      directories are returned (sorted), and the file never is.
- [x] A `delta-server` test (or an interactor test) shows the
      `GET /api/workdir/list` query parameter reaches the workspace:
      omitted → hidden excluded, set → included (`include_hidden` appears in
      `backend/crates/apps/delta-server/src/`, gated in `check_command`).
- [x] A `WorkdirPickerBody` component test shows the "Show hidden" toggle is
      off by default, and turning it on requests the listing with the hidden
      flag and renders a dot-directory row.
- [x] `scripts/e2e-real-claude.sh` no longer carries the dot-segment guard
      (`! grep -rq 'dot segment the picker cannot enter' scripts/` in
      `check_command`), while the `$HOME` guard stays.

### Manual / on-hardware (verified by a human before merge)

- [ ] From a clone under `~/.local/share/`, `make e2e-real-claude` runs the
      browser smoke to completion (needs the real, authenticated `claude`).
