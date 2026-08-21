---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: null
max_refine_rounds: 3
retries_remaining: 1
check_command: 'cd backend && cargo fmt --all -- --check && cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cd .. && BEFORE=$(find frontend/packages/gateway/wire-gen/src -type f -exec shasum -a 256 {} + | sort | shasum -a 256) && make gen && AFTER=$(find frontend/packages/gateway/wire-gen/src -type f -exec shasum -a 256 {} + | sort | shasum -a 256) && [ "$BEFORE" = "$AFTER" ] && cd frontend && pnpm -r build && pnpm -r typecheck && pnpm -r test && pnpm -r lint && cd .. && make e2e'
assignee: null
branch: task/0821-0601-refactor-sqlite-migration-ladder
created_at: 2026-08-21T06:01:04Z
updated_at: 2026-08-21T07:30:00Z
---

# refactor(sqlite): migrate an existing database forward instead of demanding a reset

## Overview

Delta's on-disk overlay currently has no upgrade path. `SCHEMA_SQL`
(`backend/crates/gateway/delta-sqlite/src/schema.rs`) creates a *fresh*
database in its final form, and the startup gate
(`SqliteStore::check_schema_version`,
`backend/crates/gateway/delta-sqlite/src/store/mod.rs:104`) compares the
binary's `SCHEMA_VERSION` against the file's `PRAGMA user_version` and
**refuses to start** on any mismatch, naming `make reset` as the remedy. A
destructive schema change therefore costs the user their entire overlay.

The policy document justifies that trade with a claim the code contradicts.
`docs/guides/compatibility.md` argues the overlay holds "only the metadata it
derives" and that "the user-visible cost of `make reset` is therefore low".
But `schema.rs:44` states the opposite: "The thread overlay — `thread_id`,
`semantic_parent_uuid`, threads, the send queue and permission history — is
**the irreplaceable data**; message content and the linear parent are a cache
rebuildable from the JSONL transcript." The `apply_additive_columns` doc
(`store/mod.rs:128`) is blunter still: the guarded `ALTER TABLE` machinery
exists "so a user is not forced to reset and lose their irreplaceable thread
overlay". The code's reading is the correct one, and delta is now in daily
real use, so `make reset` is no longer an acceptable upgrade path.

This task replaces the reset-only gate with a **migration ladder**: an ordered
list of steps, each carrying the version it produces, applied in ascending
order to whatever version the file is at. The ladder becomes the single source
of truth for the schema — `SCHEMA_SQL` is removed, and a fresh database is
built by replaying the ladder from empty, exactly like an upgraded one. There
is deliberately no second definition of the schema to keep in sync, and
therefore no equivalence test needed to police a duplication that no longer
exists. When the whole schema needs to be read at once, it is dumped from a
database the ladder built (`sqlite3 delta.db .schema`), which is true by
construction.

### Required shape of the change

1. **A `migrations` module grouped by subject, not by version.** Create
   `backend/crates/gateway/delta-sqlite/src/migrations/` with one file per
   schema subject — `session.rs`, `message.rs`, `thread.rs`, `send.rs`,
   `permission.rs`, `launch_option.rs`, `clone_root.rs`, `subagent.rs`, and
   whatever else the current `SCHEMA_SQL` defines — plus a `mod.rs` registry.
   Each file owns its subject's entire history: the step that creates it and
   every later step that alters it.

   **The design-intent commentary currently embedded in `SCHEMA_SQL` moves
   into these files' module docs** — why `session.last_activity_at` is
   denormalized and index-backed, why `transcript_path` is NULL while
   `spawning`, why child tables cascade, why every table is `STRICT`, and so
   on. None of that explanation may be lost in the move; it is the reason the
   per-subject grouping was chosen over a flat list.

2. **Steps declare their own version; the registry orders globally.** A step
   carries `to_version` and its SQL. `session.rs` may hold steps at v3 and v7
   while `clone_root.rs` holds one at v5, and the registry still applies them
   as 3, 5, 7. Within a single version, apply in the registry's declared file
   order so index and trigger steps follow their table.

3. **v3 is a squashed baseline, copied not rewritten.** Databases stamped
   `user_version = 3` already exist. Do **not** reconstruct a v1/v2/v3
   history — a reconstruction that is even slightly off would make fresh and
   existing databases diverge silently. Instead the ladder's first steps are
   all `to_version: 3` and consist of today's `SCHEMA_SQL` **split across the
   per-subject files without altering the SQL text**. A database already at 3
   skips them; a fresh database replays them. Because the baseline is the same
   statements that created the existing files, both land in the same place by
   construction. Every version from 4 onward is a genuine diff.

4. **Additive and destructive steps are distinct constructors.** Provide two
   ways to build a step — an additive one and a destructive one — so an author
   must choose at the call site rather than remember a flag. Destructive means
   a table rebuild, a rename, a drop, a tightened constraint, or any data
   movement; additive means `ADD COLUMN` and index or trigger creation. The
   distinction drives the backup rule in item 8 and nothing else.

5. **The runner takes its steps as a parameter.** Write the applier as a
   function over a step slice and a target version, not as something that
   reads the global registry directly. Tests must be able to drive it with
   synthetic ladders — including destructive steps and a step engineered to
   fail — without touching delta's real schema. The production path passes the
   real registry.

   Apply every step whose `to_version` is greater than the file's
   `user_version`, in registry order. **Each version is one transaction**, and
   `user_version` is stamped as that version's last step commits, so an
   interrupted multi-version upgrade resumes from the last version that fully
   landed rather than replaying a partial one.

6. **Registry self-validation, enforced by a test.** The registry's versions
   must be non-empty, ascending, and gap-free **from the baseline upward**, and
   the maximum `to_version` must equal `SCHEMA_VERSION`. Without this, adding a
   step and forgetting to bump `SCHEMA_VERSION` silently produces a step that is
   never applied to anything. This test guards the registry's internal
   consistency — it is not a duplication check, because there is no duplicate
   to check.

   *(Corrected during implementation: this item first demanded coverage of
   `1..=SCHEMA_VERSION`, which a squashed baseline at 3 can never satisfy — that
   requirement and item 3 contradicted each other. Relaxing it is what opened
   the hole item 7 now closes.)*

7. **Startup branches.** Replace `check_schema_version`'s four cases with:
   - `user_version == SCHEMA_VERSION` — nothing to do.
   - `user_version > SCHEMA_VERSION` — a downgrade. Keep today's hard refusal;
     the ladder only runs forward.
   - `user_version == 0` **and no `session` table** — a fresh file. Run the
     whole ladder from 0.
   - `user_version == 0` **and a `session` table exists** — a pre-gate v0.1.0
     overlay. Today this is silently stamped current, which was safe only
     because `SCHEMA_SQL` was idempotent; under the ladder the file's real
     shape is unknown and the baseline cannot be safely replayed onto it.
     **Refuse to start**, with an error naming `make reset`. Such a database
     predates the gate entirely and cannot still be in circulation.
   - `0 < user_version < the ladder's oldest step` — an overlay older than the
     squashed baseline, so no step in the ladder can bring it forward. **Refuse
     to start**, naming `make reset`. This is not hypothetical: `SCHEMA_VERSION`
     became 2 and 3 only after the latest release, so every overlay written by a
     released build is stamped 1. Applying the baseline to it would be a silent
     no-op — the steps are all `CREATE ... IF NOT EXISTS` — and would then stamp
     the file current, mislabelling an old schema as new and disarming the gate
     that used to refuse it.
   - `the ladder's oldest step <= user_version < SCHEMA_VERSION` — apply the
     pending steps.

   *(The last two cases were originally written as one, "`0 < user_version <
   SCHEMA_VERSION` — apply the pending steps". That is the hole described above;
   it was found and closed during implementation. The baseline is a floor, not
   merely a starting point.)*

8. **Back up only when a destructive step is pending.** If the pending set
   contains at least one destructive step, take a snapshot before applying
   anything:
   `VACUUM INTO 'delta.db.bak-v<user_version>'` (the source version, so a
   given database writes each such file exactly once in its life). Use
   `VACUUM INTO` rather than a file copy — the database runs in WAL mode, so a
   plain copy can miss un-checkpointed changes, while `VACUUM INTO` writes a
   consistent single-file snapshot.

   **If the target file already exists, skip the backup and proceed.** SQLite
   refuses `VACUUM INTO` onto an existing path, and a retry after a failed
   migration would otherwise be unable to start; the existing file is already
   the correct pre-migration snapshot, because the failed attempt rolled back.
   **Never delete a backup automatically** — its main value is the migration
   that appeared to succeed and is found to be wrong days later, which is
   exactly when an auto-cleanup would have removed it.

   An additive-only upgrade takes no backup and writes no file.

9. **Delete the machinery the ladder subsumes.** `SCHEMA_SQL`,
   `ADDITIVE_COLUMNS`, `AdditiveColumn`, `apply_additive_columns`,
   `column_exists`, `BACKFILL_LAST_ACTIVITY_SQL` and `RECENCY_INDEX_SQL` all
   go. The guarded-`ALTER TABLE` path existed to add a column *without* a
   version bump; under a ladder that is simply a step with a bump, so the
   detour has no remaining purpose. All ten `ADDITIVE_COLUMNS` columns fold
   into the v3 baseline as ordinary columns on their `CREATE TABLE`: an
   existing v3 database has them because `init` ran the guarded `ALTER` on
   every open, and a fresh one had them from `SCHEMA_SQL` — so both sides
   already carry them and the baseline is accurate for both.

10. **Rework the schema tests that simulated the additive path.** Five tests in
    `backend/crates/gateway/delta-sqlite/src/store/tests/schema.rs`
    (`opening_a_pre_restored_at_database_...`, `opening_a_pre_column_database_...`,
    `opening_a_pre_metadata_database_...`, `opening_a_pre_subagent_task_id_database_...`,
    `opening_a_pre_provider_database_...`) build a current database and then
    physically `DROP COLUMN` to fake an old file. Under the ladder that state
    is unreachable — a database stamped v3 has every v3 column by definition —
    so the simulation no longer describes anything real. Replace them with
    tests that exercise the genuine mechanism: build a database at the baseline,
    write rows, apply a synthetic later step, and assert the pre-existing rows
    survive and read back correctly. Do not simply delete the coverage; the
    behaviour they protect (old rows load with the new column as NULL and stay
    on their normal code path) still matters.

    `schema_gate_rescues_a_pre_gate_v0_1_0_database` must be rewritten to
    assert the new refusal from item 7.

11. **`SCHEMA_VERSION` stays at 3.** This change alters no table. An existing
    database must open with no step applied, no backup taken, and no write to
    `user_version` — the migration machinery arrives inert, which is what makes
    it safe to land before it is first needed.

12. **Documentation.** Rewrite `docs/guides/compatibility.md` "Subdomain 1 —
    SQLite schema": the policy is no longer "freely destructive, reset to
    upgrade" but "destructive changes ship a migration step; `make reset` is an
    escape hatch, not the upgrade path". Correct the claim that the reset cost
    is low — state the overlay's irreplaceable half plainly, as `schema.rs`
    already does. Remove the stale **"Implementation status. The
    `SCHEMA_VERSION` gate is scheduled in a follow-up PR"** paragraph and the
    matching "Required (scheduled)" cell in the summary table: the gate shipped
    and this task extends it. Revisit the `make reset required` commit
    convention — it should now mark the rare change that genuinely cannot be
    migrated. Add the schema-dump one-liner to the development guide as the way
    to read the whole schema at once.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] Opening a database stamped at the current `SCHEMA_VERSION` applies no
      step, writes no backup file, and leaves `user_version` untouched.
- [x] A fresh database is built by replaying the ladder and satisfies the whole
      existing `delta-sqlite` store test suite, which opens in-memory stores and
      therefore exercises the ladder-built schema on every test.
- [x] A registry test fails when the step versions have a gap, and when the
      maximum `to_version` does not equal `SCHEMA_VERSION`.
- [x] Driving the runner with a synthetic ladder migrates a database from an
      older version to a newer one and pre-existing rows survive with the newly
      added column reading as NULL.
- [x] A synthetic ladder whose step fails leaves `user_version` unchanged and
      the data intact — the transaction rolled back.
- [x] A multi-version synthetic ladder stamps each version as it lands, so an
      upgrade interrupted after version N resumes at N rather than replaying it.
- [x] A pending set containing a destructive step writes
      `delta.db.bak-v<source>`; an additive-only pending set writes no file.
- [x] A pending destructive upgrade whose backup file already exists proceeds
      rather than failing.
- [x] A database whose `user_version` exceeds `SCHEMA_VERSION` is refused with
      the documented error.
- [x] A `user_version == 0` database that already has a `session` table is
      refused with the documented error naming `make reset`.
- [x] A database stamped below the ladder's oldest step is refused with the
      documented error, its `user_version` is left untouched, and no backup is
      written.
- [x] `grep` finds no remaining reference to `SCHEMA_SQL`, `ADDITIVE_COLUMNS`,
      or `apply_additive_columns` anywhere in the tree.

### Manual / on-hardware (verified by a human before merge)

- [ ] The real development database opens unchanged against the new binary —
      the session list, threads, and pending sends are all still there, and no
      `delta.db.bak-*` file was created.

## Out of scope

- **Bumping `SCHEMA_VERSION` or shipping any actual v4 step.** This task builds
  the mechanism and lands it inert. The first real migration rides a later
  change that needs one.
- **Backfilling a v1/v2/v3 history.** v3 is a squashed baseline by design; see
  item 3 for why reconstructing the history would be actively harmful.
- **A scheduled or continuous backup regime for `delta.db`.** The backup here
  is scoped to destructive migrations. Whether delta should back the overlay up
  on any other occasion is a separate question this task does not answer.
- **A downgrade path.** The ladder runs forward only; running an older binary
  against a newer database stays a hard refusal.
- **Building any schema-dump tooling.** `sqlite3 delta.db .schema` already does
  it; the guide gains the one-liner, not a new command or `make` target.
