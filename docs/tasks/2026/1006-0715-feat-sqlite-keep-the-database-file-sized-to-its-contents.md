---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && git grep -q 'auto_vacuum' -- backend/crates/gateway/delta-sqlite/src/store && git grep -q 'journal_size_limit' -- backend/crates/gateway/delta-sqlite/src/store && git grep -qi 'auto_vacuum' -- docs/guides/install/README.md"
assignee: null
branch: task/1006-0715-feat-sqlite-keep-the-database-file-sized-to-its-contents
created_at: 2026-10-05T22:11:49Z
updated_at: 2026-10-05T22:35:41Z
---

# feat(sqlite): keep the database file sized to its contents

## Overview

Deleting rows from `delta.db` never makes the file smaller. SQLite only moves
the freed pages onto its free list and reuses them later; the file keeps its
high-water mark. The store opens the database in WAL mode
(`backend/crates/gateway/delta-sqlite/src/store/mod.rs`, `Store::init`) and
nothing ever runs `VACUUM` or checkpoints the WAL with truncation, so the
`-wal` file also stays at the largest size it ever reached. The Storage view
now shows the database size, and session cleanup will delete rows in bulk;
with the current settings a user who deletes a hundred sessions would see
the number not move.

Rather than adding a "compact" step that every deleting code path has to
remember, make it an invariant of the connection: the file size reflects
its contents at all times.

### Change

- In `Store::init`, after `journal_mode = WAL` and before the migration
  ladder runs, read `PRAGMA auto_vacuum`. If it is not `1` (FULL; `2` is INCREMENTAL, which
  frees nothing until `incremental_vacuum` is called), run
  `PRAGMA auto_vacuum = FULL` followed by `VACUUM` so the setting takes
  effect on an existing file (SQLite only honours a changed `auto_vacuum`
  after a `VACUUM`, which rebuilds the file with the pointer-map pages the
  mode needs). This is a one-time conversion for databases created before
  this change and a no-op afterwards. It must run **outside** any
  transaction — `VACUUM` refuses to run inside one — which is why it is
  connection setup and not a step in the migration ladder
  (`migrations/runner.rs` wraps every version in a transaction). It changes
  no schema and no row, so it neither bumps `SCHEMA_VERSION` nor takes a
  pre-migration snapshot; say so in a comment at the call site.
- Set `PRAGMA journal_size_limit` on every connection (a few MiB; pick a
  value and justify it in the comment) so that after a checkpoint the WAL
  file is truncated to that limit instead of staying at its high-water
  mark.
- Skip the conversion for an in-memory database (`open_in_memory`), where
  there is no file to size; the pragmas are harmless there but the
  `VACUUM` is wasted work. Keep the in-memory path on the same ladder
  otherwise.
- Document the behaviour in one place: the install guide's "Where the app
  keeps its data" section (`docs/guides/install/README.md`) already lists
  `delta.db`, `delta.db-wal` and `delta.db-shm`; add one sentence that the
  database shrinks as sessions are deleted (`auto_vacuum`), and that the
  WAL is capped after checkpoints. Do not describe the migration mechanics
  there.

### Tests

- A store test (next to `backend/crates/gateway/delta-sqlite/src/store/tests/schema.rs`)
  that opens a file-backed store, asserts `PRAGMA auto_vacuum` reads `1`,
  inserts enough rows to grow the file by several pages, deletes them, and
  asserts `PRAGMA page_count` (or the file length) is smaller afterwards
  than at its peak. Use a `tempfile` directory, as the existing file-backed
  tests do.
- A test that opens an existing file whose `auto_vacuum` is `0` (create it
  with a raw `rusqlite::Connection` first), reopens it through the store,
  and asserts it now reports `1` and still passes the schema gate.
- A test that `PRAGMA journal_size_limit` on an opened store returns the
  configured value.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `Store::init` configures `auto_vacuum` and `journal_size_limit`
      (`git grep` gates in `check_command`), and the install guide mentions
      `auto_vacuum` (gate).
- [x] A test shows a file-backed store reports `PRAGMA auto_vacuum = 1`
      and that deleting rows reduces `page_count` below its peak.
- [x] A test shows a pre-existing database with `auto_vacuum = 0` is
      converted on open and still passes the schema gate.
- [x] A test shows `journal_size_limit` is set on an opened store.
- [x] `make check` passes (the e2e-fake suite opens real file-backed
      databases through this path).

### Before merge (verified outside the check command)

- [x] On the development machine, a copy of a Delta database created before
      this change (223 MB, `auto_vacuum = 0`, schema v9) was opened through
      the new server once: it opened, `sqlite3 delta.db 'PRAGMA auto_vacuum'`
      reported `1` afterwards (schema at v10, rows intact), and
      `GET /api/storage` reported it with its size.
