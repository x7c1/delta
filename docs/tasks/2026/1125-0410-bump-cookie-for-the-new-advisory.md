---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: null
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && (cd backend && cargo deny check advisories) && ! grep -A1 '^name = \"cookie\"' backend/Cargo.lock | grep -q 'version = \"0.18.2\"'"
assignee: null
branch: task/1125-0410-bump-cookie-for-the-new-advisory
created_at: 2026-11-24T19:10:46Z
updated_at: 2026-11-24T19:23:17Z
---

# build(deps): bump cookie to 0.18.3 for the new advisory

## Overview

`cargo deny check advisories` (`make audit`, the CI `audit` job) now fails on
every pull request: a new advisory reports that `cookie` 0.18.2 can reuse a
nonce for private cookies after a `fork`, fixed in 0.18.3. Delta does not
depend on `cookie` directly; `backend/Cargo.lock` pins 0.18.2 through `tauri`,
`tauri-runtime` and `wry` (the desktop shell).

Resolve it by upgrading the locked version (`cargo update -p cookie` in
`backend/`), not by adding the advisory to the `ignore` list in
`backend/deny.toml`. Do not add an `ignore` entry for it. Keep the update to
`cookie` (and only what its 0.18.3 release itself requires); do not update
unrelated crates.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `backend/Cargo.lock` no longer locks `cookie` 0.18.2 (gate in
      `check_command`).
- [x] `cargo deny check advisories` passes in `backend/` (gate in
      `check_command`).
