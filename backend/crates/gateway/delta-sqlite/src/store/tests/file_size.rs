//! The connection settings that keep the database file sized to its contents:
//! `auto_vacuum = FULL`, the one-time conversion of a file created without it,
//! and the WAL size limit.

use rusqlite::Connection;

use super::super::{SqliteStore, AUTO_VACUUM_FULL, JOURNAL_SIZE_LIMIT_BYTES};
use super::new_session;
use crate::SCHEMA_VERSION;

async fn pragma_i64(store: &SqliteStore, pragma: &str) -> i64 {
    let conn = store.conn.lock().await;
    conn.query_row(&format!("PRAGMA {pragma}"), [], |row| row.get(0))
        .unwrap()
}

/// A file-backed store runs in `auto_vacuum = FULL`, so deleting rows gives
/// their pages back to the file system instead of parking them on a free list.
#[tokio::test]
async fn deleting_rows_shrinks_a_file_backed_store() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("delta.db");
    let store = SqliteStore::open(path.to_str().unwrap()).unwrap();

    assert_eq!(pragma_i64(&store, "auto_vacuum").await, AUTO_VACUUM_FULL);
    let initial = pragma_i64(&store, "page_count").await;

    {
        let conn = store.conn.lock().await;
        // Each body is larger than a page, so 64 rows grow the file by well
        // over 64 pages.
        let body = "x".repeat(8 * 1024);
        for i in 0..64 {
            conn.execute(
                "INSERT INTO prompt_template (label, text, created_at, updated_at)
                 VALUES (?1, ?2, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                rusqlite::params![format!("t-{i}"), body],
            )
            .unwrap();
        }
    }
    let peak = pragma_i64(&store, "page_count").await;
    assert!(
        peak > initial + 64,
        "the inserts grow the file (initial {initial}, peak {peak})"
    );

    {
        let conn = store.conn.lock().await;
        conn.execute("DELETE FROM prompt_template", []).unwrap();
    }
    let after = pragma_i64(&store, "page_count").await;
    assert!(
        after < peak,
        "the file shrinks once the rows are gone (peak {peak}, after {after})"
    );
    assert_eq!(
        pragma_i64(&store, "freelist_count").await,
        0,
        "no freed page is left parked on the free list"
    );
}

/// A database created before the setting existed (`auto_vacuum = 0`) is
/// converted on open, keeps its rows, and still passes the schema gate.
#[tokio::test]
async fn a_database_without_auto_vacuum_is_converted_on_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("delta.db");
    let path_str = path.to_str().unwrap();

    {
        let store = SqliteStore::open(path_str).unwrap();
        store.register_session(new_session()).await.unwrap();
    }
    {
        // Put the file back the way an older binary left it.
        let conn = Connection::open(path_str).unwrap();
        conn.execute_batch("PRAGMA auto_vacuum = 0; VACUUM;")
            .unwrap();
        let mode: i64 = conn
            .query_row("PRAGMA auto_vacuum", [], |row| row.get(0))
            .unwrap();
        assert_eq!(mode, 0, "the fixture starts without auto_vacuum");
    }

    let store = SqliteStore::open(path_str).unwrap();

    assert_eq!(pragma_i64(&store, "auto_vacuum").await, AUTO_VACUUM_FULL);
    assert_eq!(
        pragma_i64(&store, "user_version").await,
        i64::from(SCHEMA_VERSION),
        "the conversion leaves the schema stamp alone"
    );
    assert!(
        store.session(&"sess-1".into()).await.unwrap().is_some(),
        "the rows survive the conversion"
    );
}

/// Every opened store caps the WAL file at the configured size after a
/// checkpoint.
#[tokio::test]
async fn an_opened_store_sets_the_journal_size_limit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("delta.db");
    let store = SqliteStore::open(path.to_str().unwrap()).unwrap();
    assert_eq!(
        pragma_i64(&store, "journal_size_limit").await,
        JOURNAL_SIZE_LIMIT_BYTES
    );

    let in_memory = SqliteStore::open_in_memory().unwrap();
    assert_eq!(
        pragma_i64(&in_memory, "journal_size_limit").await,
        JOURNAL_SIZE_LIMIT_BYTES
    );
}
