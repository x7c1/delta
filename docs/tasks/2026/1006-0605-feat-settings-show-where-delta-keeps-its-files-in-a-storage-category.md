---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, user-experience, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && git grep -q 'GetStorage' -- backend/crates/gateway/delta-wire/src/endpoint/table.rs && git grep -q \"'storage'\" -- frontend/packages/apps/web/src/store/settingsStore.ts && git grep -q '/api/storage' -- docs/guides/api/settings.md && test -f backend/crates/gateway/delta-wire/src/rest/storage_response.rs"
assignee: null
branch: task/1006-0605-feat-settings-show-where-delta-keeps-its-files-in-a-storage-category
created_at: 2026-10-05T21:02:11Z
updated_at: 2026-10-05T22:00:20Z
---

# feat(settings): show where Delta keeps its files in a Storage category

## Overview

A user cannot see where a running Delta keeps its files. The only thing the
HTTP API exposes about the server's own state is `GET /api/version`; the
database path, the data directory, the tmux socket name, the worktree base
and the settings and tmux configuration files are resolved at startup and
logged to stderr, which the desktop app never shows. The install guide lists
the locations in prose, but a user with a question about *this* install
("which directory is it actually using? how big is the database?") has no
answer inside the app.

The server now derives every path it writes from one value,
`delta_bootstrap::DataLayout` (`backend/crates/libs/delta-bootstrap/src/data_layout.rs`),
built from `Config::data_dir`; worktrees stay outside it under
`Config::worktree_base`. This task exposes that inventory over one endpoint
and shows it in a new **Storage** category of the Settings dialog. The
category's scope is "what Delta stores on this machine and how much"; later
work adds the cleanup and erase actions to the same category, so this task
builds the category and the read-only inventory only.

### Backend

- Add `GET /api/storage` → `WireStorageResponse`. Declare it in
  `backend/crates/gateway/delta-wire/src/endpoint/table.rs` (next to
  `GetVersion`, with a doc comment in the same style), define the wire type
  in its own module `backend/crates/gateway/delta-wire/src/rest/storage_response.rs`
  (one public type per module, `mod` + `pub use` paired in `rest/mod.rs`,
  `#[ts(rename = "StorageResponse")]`), bind it in
  `backend/crates/apps/delta-server/src/app/mod.rs`, implement the handler
  in `delta-server/src/api/` (follow how `get_version` is organised; if
  `api/mod.rs` is already a large flat module, put the handler in its own
  file), and run `make gen` so `@delta/wire-gen` carries the TypeScript
  binding (`gen-check` in `make check` fails otherwise).
- The response lists, with absolute paths:
  - `identifier` and `version` (the same string `GET /api/version` returns);
  - `data_dir`;
  - `database`: path plus `bytes` = the sizes of `delta.db`, `delta.db-wal`
    and `delta.db-shm` that exist, summed (a user sees one number for "the
    database");
  - `snapshots`: every `delta.db.bak-v<N>` beside the database, each with
    path and `bytes` (empty list when none);
  - `hook_state` path, `sessions_dir`, `session_settings` path (for the
    current port), `tmux_conf` path, `tmux_socket` (the socket *name*, as
    passed to `tmux -L`), `worktree_base`, `transcript_root`.
  - **Never** the hook secret or the auth token. Add a unit test that
    serialises a response and asserts no key named `hook_secret`, `secret`
    or `auth_token` appears.
- `AppState` (`backend/crates/apps/delta-server/src/state.rs`) keeps only
  the tmux socket, auth token and hook secret from `Config`; thread a
  read-only inventory value into it (built in `AppState::build` from the
  `Config`'s `DataLayout`, `worktree_base`, `transcript_root`, `identifier`
  and `port`) so the handler needs no global. File sizes are read at request
  time (`std::fs::metadata`), not cached; a missing optional file (`-wal`,
  `-shm`, snapshots) contributes zero and no error.
- Document the endpoint in `docs/guides/api/settings.md` under a new
  `## Storage` section modelled on `## Server version`, with an example
  body.

### Frontend

- `frontend/packages/gateway/api-client`: `getStorage()` in `http.ts`,
  `queryKeys.storage` in `query-keys.ts`, `useStorageQuery` in
  `query-hooks.ts` (same shape as `useVersionQuery`, but with a finite
  `staleTime` such as 30 s so sizes refresh when the user reopens the
  category).
- `frontend/packages/testing/api-mocks/src/handlers.ts`: a `*/api/storage`
  handler returning a fixed inventory with placeholder paths (e.g.
  `/home/u/.local/share/io.github.x7c1.delta/...`), so mock mode and the
  Playwright mock suite have data.
- `frontend/packages/apps/web/src/store/settingsStore.ts`: add `'storage'`
  to `SettingsCategoryId` and `SETTINGS_CATEGORY_IDS`.
- `frontend/packages/apps/web/src/features/settings/SettingsView.tsx`: add a
  registry entry `{ id: 'storage', label: 'Storage', render: (active) =>
  <StorageSection active={active} /> }`. `SettingsView.tsx` is already
  1,868 lines with every section inline; put the new section in its own
  module under `features/settings/` (for example
  `features/settings/storage/StorageSection.tsx`) rather than appending to
  it, and fetch only while `active` like `CloneRootsSection` does.
- The section shows one row per inventory item: a short label, the absolute
  path (monospace, truncated from the left on overflow like `displayPath`
  does elsewhere), the size where there is one (humanised, e.g. `3.1 MB`,
  with the exact byte count in a tooltip), and a copy-to-clipboard control
  per path. Snapshots are a sub-list under the database row; when there
  are none, say so in one line rather than hiding the row. Show `identifier`
  and `version` at the top. Loading and error states follow the other
  sections. No actions in this task — no delete, no "reveal in file
  manager".
- Tests: a vitest for the section next to `SettingsView.test.tsx` (renders
  rows from a mocked query; the no-snapshots line; size formatting), and one
  Playwright spec in the mock suite that opens Settings, selects Storage and
  sees the mocked data directory path.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `GetStorage` is declared in the endpoint table, the wire type lives in
      `rest/storage_response.rs`, the TypeScript binding is regenerated
      (`gen-check` passes inside `make check`), and `docs/guides/api/settings.md`
      documents `/api/storage` (`git grep` and `test -f` gates in
      `check_command`; all fail on the current tree).
- [x] A unit test asserts the serialised `WireStorageResponse` contains no
      `hook_secret`, `secret` or `auth_token` key.
- [x] A handler/endpoint test (in `delta-server/src/app/tests/`) shows
      `GET /api/storage` returns the data directory the test configured,
      a database size that reflects the files on disk, and an empty
      snapshot list when none exist.
- [x] `'storage'` is a `SettingsCategoryId` and the Storage section renders
      the mocked inventory (vitest), and the Playwright mock spec finds the
      mocked data directory path after selecting Storage.
- [x] `make check` passes.

### Before merge (verified outside the check command)

- [x] On a macOS development machine, every path `GET /api/storage` reports
      exists on disk under the configured data directory, and the database
      size equals the summed file lengths of `delta.db`, `delta.db-wal` and
      `delta.db-shm` (`stat -f %z`; not `du`, which counts disk blocks and
      would also pick up the snapshots). Verified against a server booted on
      a throwaway `DELTA_DATA_DIR` (snapshots empty, sizes equal).
- [ ] The same check on an Ubuntu install. Needs a Linux machine; not run
      before merge.
- [x] The identifier alone selects the data directory: a server started with
      only `DELTA_IDENTIFIER` set reports `~/Library/Application Support/<identifier>/`
      as its data directory (mode `0700`, database present) — the path
      `make dev` takes with the `.dev` identifier.
