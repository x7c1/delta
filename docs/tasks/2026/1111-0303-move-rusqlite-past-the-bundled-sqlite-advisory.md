---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: null
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && (cd backend && cargo deny check advisories) && grep -qE '^rusqlite = \\{ version = \"0\\.41\"' backend/Cargo.toml && ! grep -qE '^ignore = \\[[^]]*RUSTSEC-2026-0459' backend/deny.toml && ! grep -q 'RUSTSEC-2026-0459' backend/deny.toml"
assignee: null
branch: task/1111-0303-move-rusqlite-past-the-bundled-sqlite-advisory
created_at: 2026-11-10T18:03:55Z
updated_at: 2026-11-10T18:21:22Z
---

# fix(backend): move rusqlite to 0.41 to leave RUSTSEC-2026-0459 behind

## Overview

CI's `audit` job (`make audit`, which runs `cargo deny check advisories` in
`backend/`) fails on `main` with RUSTSEC-2026-0459: the SQLite that
`libsqlite3-sys` 0.30 bundles can return uninitialized memory when it reads a
crafted `-wal` file. The workspace pins `rusqlite = { version = "0.32",
features = ["bundled"] }` in `backend/Cargo.toml`, and rusqlite 0.32 pulls
`libsqlite3-sys` 0.30.1. rusqlite 0.41 bundles `libsqlite3-sys` 0.39, which
carries a fixed SQLite. Until this lands, every pull request's `audit` check
fails, including ones that do not touch the backend.

What to do:

1. **Bump `rusqlite`** in `backend/Cargo.toml` to `0.41` (newest 0.41.x),
   keeping the `bundled` feature, and update `backend/Cargo.lock` with
   `cargo update -p rusqlite` (or the minimal update that resolves). The lock
   should change only by what the upgrade pulls in.
2. **Adapt `delta-sqlite`** (`backend/crates/gateway/delta-sqlite/`) to the
   rusqlite API changes between 0.32 and 0.41. A known one: rusqlite 0.41
   implements `FromSql`/`ToSql` for `u64` (and `usize`) only behind its
   `fallible_uint` feature, so any column read or bound as `u64`/`usize` has to
   go through `i64` with a checked conversion (`u64::try_from`, mapping a
   failure to `rusqlite::Error::IntegralValueOutOfRange` or the crate's own
   error). Prefer that over enabling new rusqlite features, so the workspace's
   rusqlite features stay as they were. Fix the cause of each compile error;
   do not add `allow` attributes.
3. **Keep the schema, the migrations and the on-disk layout unchanged.** The
   migrations under `delta-sqlite/src/migrations/` must produce the same
   database as before; the existing migration tests guard this.
4. **Do not waive the advisory.** `backend/deny.toml` must not gain an
   `ignore` entry for RUSTSEC-2026-0459; the fix is the upgrade.
5. **Update docs** that name the rusqlite version, if any.

Out of scope: other dependency updates (`glib` 0.20 is a separate change).

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `backend/Cargo.toml` declares `rusqlite` `0.41` with the `bundled`
      feature (grep gate in `check_command`)
- [x] `cargo deny check advisories` passes in `backend/` without
      RUSTSEC-2026-0459 being named in `backend/deny.toml` (both in
      `check_command`)
- [x] `make check` passes with rusqlite 0.41, including the `delta-sqlite`
      migration and store tests
