//! Ladder tests: the registry's own consistency, and the runner driven with
//! synthetic ladders.
//!
//! The runner takes its steps as a parameter precisely so these can exercise
//! multi-version upgrades, a step engineered to fail, and destructive steps
//! without touching delta's real schema — the production ladder is validated
//! here too, but never mutated.

use rusqlite::Connection;

use super::{migrate, registry, validate, Step};
use crate::error::Error;
use crate::SCHEMA_VERSION;

/// The synthetic baseline every runner test starts from: one table at v3, so
/// the ladder under test has somewhere to add to.
const BASELINE: &str = "\
CREATE TABLE note (
  id   INTEGER PRIMARY KEY,
  body TEXT NOT NULL
) STRICT;";

fn baseline_step() -> Step {
    Step::additive(3, BASELINE)
}

fn user_version(conn: &Connection) -> u32 {
    conn.query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap()
}

fn column_names(conn: &Connection, table: &str) -> Vec<String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .unwrap();
    let names = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    names
}

/// A database at the synthetic baseline (v3) carrying one row.
fn database_at_baseline() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    migrate(&conn, &[baseline_step()], 0, 3, None).unwrap();
    conn.execute_batch("INSERT INTO note (id, body) VALUES (1, 'kept');")
        .unwrap();
    conn
}

// --- the registry's own consistency -----------------------------------------

#[test]
fn the_registry_agrees_with_the_schema_version() {
    let steps = registry();
    validate(&steps, SCHEMA_VERSION).expect("the shipped ladder must be consistent");
    assert_eq!(
        steps.last().expect("the ladder has steps").to_version,
        SCHEMA_VERSION,
        "the last step must produce the version the binary expects",
    );
}

#[test]
fn the_registry_orders_steps_by_version_keeping_declaration_order_within_one() {
    let steps = registry();
    assert!(
        steps.windows(2).all(|w| w[0].to_version <= w[1].to_version),
        "the registry must hand the runner an ascending ladder",
    );
    // The v3 baseline is the ladder's lowest version, and `session` is declared
    // first, so its `CREATE TABLE` must be the very first statement replayed —
    // every other subject references it.
    assert!(
        steps[0].sql.contains("CREATE TABLE IF NOT EXISTS session"),
        "the baseline must create `session` before the tables that reference it",
    );
}

#[test]
fn a_gap_between_versions_fails_validation() {
    // 3 -> 5 with nothing at 4: version 4 is a rung the runner would have to
    // stand on and cannot.
    let gapped = [
        baseline_step(),
        Step::additive(5, "ALTER TABLE note ADD COLUMN tag TEXT;"),
    ];
    let err = validate(&gapped, 5).expect_err("a gapped ladder must be rejected");
    let rendered = err.to_string();
    assert!(
        matches!(err, Error::InvalidLadder(_)),
        "expected InvalidLadder, got {rendered}",
    );
    assert!(
        rendered.contains("version 4 has no steps"),
        "the error must name the missing version: {rendered}",
    );
}

#[test]
fn a_ladder_whose_top_disagrees_with_the_expected_version_fails_validation() {
    // The forgotten-bump case: a step at 4 while the binary still expects 3, so
    // the step would sit above the target version and never be applied.
    let unbumped = [
        baseline_step(),
        Step::additive(4, "ALTER TABLE note ADD COLUMN tag TEXT;"),
    ];
    let err = validate(&unbumped, 3).expect_err("an unbumped ladder must be rejected");
    let rendered = err.to_string();
    assert!(
        matches!(err, Error::InvalidLadder(_)),
        "expected InvalidLadder, got {rendered}",
    );
    assert!(
        rendered.contains("produces version 4")
            && rendered.contains("expected schema version is 3"),
        "the error must name both versions: {rendered}",
    );
}

#[test]
fn an_empty_ladder_fails_validation() {
    let err = validate(&[], 3).expect_err("an empty ladder must be rejected");
    assert!(err.to_string().contains("no steps"), "{err}");
}

// --- the shipped ladder's own steps ------------------------------------------

#[test]
fn the_v6_step_renames_the_send_hold_marker_and_carries_its_values_over() {
    // The real registry, not a synthetic ladder: a database built to v5 holds
    // the marker as `restored_at`, and the pending v6 step has to move the
    // column *and* everything already stored in it.
    let conn = Connection::open_in_memory().unwrap();
    let steps = registry();
    migrate(&conn, &steps, 0, 5, None).unwrap();

    // Written straight through SQL rather than through the store: the store's
    // send writes speak the *current* column name, and the point here is the
    // shape that predates it.
    conn.execute_batch(
        "INSERT INTO session (id, cwd, status, created_at)
         VALUES ('sess-1', '/work', 'active', '2026-01-01T00:00:00Z');
         INSERT INTO thread (id, session_id, title, created_at)
         VALUES (1, 'sess-1', 'main', '2026-01-01T00:00:00Z');
         INSERT INTO send (id, session_id, thread_id, text, status, created_at, restored_at)
         VALUES (1, 'sess-1', 1, 'held',  'queued', '2026-01-01T00:00:00Z', '2026-01-02T00:00:00Z'),
                (2, 'sess-1', 1, 'plain', 'queued', '2026-01-01T00:00:00Z', NULL);",
    )
    .unwrap();

    migrate(&conn, &steps, 5, 6, None).unwrap();

    assert_eq!(user_version(&conn), 6);
    let columns = column_names(&conn, "send");
    assert!(
        columns.contains(&"held_at".to_owned()),
        "the marker is readable under its new name: {columns:?}",
    );
    assert!(
        !columns.contains(&"restored_at".to_owned()),
        "and no longer under the old one: {columns:?}",
    );

    let marked: Option<String> = conn
        .query_row("SELECT held_at FROM send WHERE id = 1", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(
        marked.as_deref(),
        Some("2026-01-02T00:00:00Z"),
        "a row already held keeps its stamp — the rename backfills nothing",
    );
    let unmarked: Option<String> = conn
        .query_row("SELECT held_at FROM send WHERE id = 2", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(unmarked, None, "and an unheld row stays unheld");
}

// --- the runner --------------------------------------------------------------

#[test]
fn nothing_pending_leaves_the_database_untouched() {
    let conn = database_at_baseline();
    migrate(&conn, &[baseline_step()], 3, 3, None).unwrap();
    assert_eq!(user_version(&conn), 3, "the stamp is not rewritten");
    assert_eq!(column_names(&conn, "note"), ["id", "body"]);
}

#[test]
fn a_later_step_migrates_an_existing_database_and_keeps_its_rows() {
    let conn = database_at_baseline();
    let ladder = [
        baseline_step(),
        Step::additive(4, "ALTER TABLE note ADD COLUMN tag TEXT;"),
    ];

    migrate(&conn, &ladder, 3, 4, None).unwrap();

    assert_eq!(user_version(&conn), 4);
    let (body, tag): (String, Option<String>) = conn
        .query_row("SELECT body, tag FROM note WHERE id = 1", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .unwrap();
    assert_eq!(body, "kept", "a pre-existing row survives the migration");
    assert_eq!(tag, None, "the newly added column reads NULL on an old row");
}

#[test]
fn a_failing_step_rolls_its_whole_version_back() {
    let conn = database_at_baseline();
    // The version's second statement violates NOT NULL, so the whole
    // transaction — the added column included — must roll back.
    let ladder = [
        baseline_step(),
        Step::additive(
            4,
            "ALTER TABLE note ADD COLUMN tag TEXT;\n\
             INSERT INTO note (id, body) VALUES (2, NULL);",
        ),
    ];

    migrate(&conn, &ladder, 3, 4, None).expect_err("the failing step must surface as an error");

    assert_eq!(user_version(&conn), 3, "the version is not stamped");
    assert_eq!(
        column_names(&conn, "note"),
        ["id", "body"],
        "the half-applied column is rolled back",
    );
    let bodies: Vec<String> = conn
        .prepare("SELECT body FROM note ORDER BY id")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(bodies, ["kept"], "the pre-existing data is intact");
}

#[test]
fn each_version_is_stamped_as_it_lands_so_an_interrupted_upgrade_resumes() {
    let conn = database_at_baseline();
    let ladder = [
        baseline_step(),
        Step::additive(4, "ALTER TABLE note ADD COLUMN tag TEXT;"),
        Step::additive(5, "ALTER TABLE note ADD COLUMN colour TEXT;"),
    ];

    // Stop after version 4, standing in for a crash between the two versions.
    migrate(&conn, &ladder, 3, 4, None).unwrap();
    assert_eq!(user_version(&conn), 4, "version 4 landed and was stamped");
    assert_eq!(column_names(&conn, "note"), ["id", "body", "tag"]);

    // Resuming from the stamped version replays nothing: re-running the v4 step
    // would fail outright, since SQLite refuses a duplicate column name.
    migrate(&conn, &ladder, user_version(&conn), 5, None).unwrap();
    assert_eq!(user_version(&conn), 5);
    assert_eq!(column_names(&conn, "note"), ["id", "body", "tag", "colour"]);
}

#[test]
fn a_later_version_failing_keeps_the_version_that_already_landed() {
    let conn = database_at_baseline();
    let ladder = [
        baseline_step(),
        Step::additive(4, "ALTER TABLE note ADD COLUMN tag TEXT;"),
        Step::additive(5, "ALTER TABLE nonexistent ADD COLUMN colour TEXT;"),
    ];

    migrate(&conn, &ladder, 3, 5, None).expect_err("version 5 must fail");

    assert_eq!(user_version(&conn), 4, "version 4 stays landed and stamped");
    assert_eq!(column_names(&conn, "note"), ["id", "body", "tag"]);
}

// --- the pre-migration backup ------------------------------------------------

/// A file-backed database at the synthetic baseline, in WAL mode like the real
/// store, carrying one row. Returns the temp dir (kept alive by the caller) and
/// the open connection.
fn file_database_at_baseline() -> (tempfile::TempDir, String, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("delta.db").to_str().unwrap().to_owned();
    let conn = Connection::open(&path).unwrap();
    let _mode: String = conn
        .query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
        .unwrap();
    migrate(&conn, &[baseline_step()], 0, 3, None).unwrap();
    conn.execute_batch("INSERT INTO note (id, body) VALUES (1, 'kept');")
        .unwrap();
    (dir, path, conn)
}

/// A destructive step: the table rebuild SQLite forces on any constraint edit.
const REBUILD_NOTE: &str = "\
CREATE TABLE note_new (
  id   INTEGER PRIMARY KEY,
  body TEXT NOT NULL,
  tag  TEXT NOT NULL DEFAULT ''
) STRICT;
INSERT INTO note_new (id, body) SELECT id, body FROM note;
DROP TABLE note;
ALTER TABLE note_new RENAME TO note;";

#[test]
fn a_destructive_pending_set_snapshots_the_database_first() {
    let (dir, path, conn) = file_database_at_baseline();
    let ladder = [baseline_step(), Step::destructive(4, REBUILD_NOTE)];

    migrate(&conn, &ladder, 3, 4, Some(&path)).unwrap();

    let backup = format!("{path}.bak-v3");
    assert!(
        std::path::Path::new(&backup).exists(),
        "a destructive upgrade writes `<db>.bak-v<source version>`",
    );
    // The snapshot is a usable database holding the *pre*-migration shape.
    let snapshot = Connection::open(&backup).unwrap();
    assert_eq!(user_version(&snapshot), 3);
    assert_eq!(column_names(&snapshot, "note"), ["id", "body"]);
    let body: String = snapshot
        .query_row("SELECT body FROM note WHERE id = 1", [], |row| row.get(0))
        .unwrap();
    assert_eq!(body, "kept");

    // And the migrated database really did move on.
    assert_eq!(user_version(&conn), 4);
    assert_eq!(column_names(&conn, "note"), ["id", "body", "tag"]);
    drop(dir);
}

#[test]
fn an_additive_only_pending_set_writes_no_backup() {
    let (dir, path, conn) = file_database_at_baseline();
    let ladder = [
        baseline_step(),
        Step::additive(4, "ALTER TABLE note ADD COLUMN tag TEXT;"),
    ];

    migrate(&conn, &ladder, 3, 4, Some(&path)).unwrap();

    assert_eq!(user_version(&conn), 4);
    assert!(
        !std::path::Path::new(&format!("{path}.bak-v3")).exists(),
        "an additive-only upgrade must not write a snapshot",
    );
    drop(dir);
}

/// A replay from 0 builds a fresh database rather than changing one, so its
/// destructive steps snapshot nothing — otherwise every fresh install would
/// find a `<db>.bak-v0` copy of an empty database next to its new one.
#[test]
fn a_replay_onto_a_fresh_file_writes_no_backup() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fresh.db").to_str().unwrap().to_owned();
    let conn = Connection::open(&path).unwrap();
    let ladder = [baseline_step(), Step::destructive(4, REBUILD_NOTE)];

    migrate(&conn, &ladder, 0, 4, Some(&path)).unwrap();

    assert_eq!(user_version(&conn), 4, "the whole ladder replayed");
    assert!(
        !std::path::Path::new(&format!("{path}.bak-v0")).exists(),
        "a fresh file has nothing to snapshot",
    );
    drop(dir);
}

#[test]
fn an_existing_snapshot_is_kept_and_the_migration_proceeds() {
    let (dir, path, conn) = file_database_at_baseline();
    let backup = format!("{path}.bak-v3");
    // Stand in for the snapshot a previous, failed attempt already wrote.
    // SQLite refuses `VACUUM INTO` onto an existing path, so a retry would be
    // unable to start at all if the runner insisted on writing it again.
    std::fs::write(&backup, b"an earlier snapshot").unwrap();

    let ladder = [baseline_step(), Step::destructive(4, REBUILD_NOTE)];
    migrate(&conn, &ladder, 3, 4, Some(&path)).unwrap();

    assert_eq!(user_version(&conn), 4, "the retry gets through");
    assert_eq!(
        std::fs::read(&backup).unwrap(),
        b"an earlier snapshot",
        "the existing snapshot is left exactly as it was",
    );
    drop(dir);
}

#[test]
fn an_in_memory_database_takes_no_snapshot() {
    let conn = database_at_baseline();
    let ladder = [baseline_step(), Step::destructive(4, REBUILD_NOTE)];

    // `db_path: None` — there is no file to snapshot, and the runner must say so
    // rather than let `VACUUM INTO` invent one.
    migrate(&conn, &ladder, 3, 4, None).unwrap();

    assert_eq!(user_version(&conn), 4);
    assert_eq!(column_names(&conn, "note"), ["id", "body", "tag"]);
}
