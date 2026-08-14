//! [`SqliteStore`]: the concrete [`SessionStore`](delta_usecase::SessionStore).
//!
//! Split into one file per aggregate: this module holds the store struct,
//! the open/migration path, and the helpers the aggregates share; the
//! per-aggregate modules hold the methods (as inherent twins of the trait
//! methods) plus their row mappers and column lists; `session_store` wires
//! the [`SessionStore`](delta_usecase::SessionStore) trait up by forwarding
//! to the twins.

mod clone_roots;
mod launch_options;
mod messages;
mod permissions;
mod sends;
mod session_store;
mod sessions;
mod subagents;
mod threads;

#[cfg(test)]
mod tests;

use rusqlite::{params, Connection, OptionalExtension};
use tokio::sync::Mutex;

use delta_model::{SessionId, ThreadId};

use crate::error::{Error, Result};
use crate::schema::{
    ADDITIVE_COLUMNS, BACKFILL_LAST_ACTIVITY_SQL, RECENCY_INDEX_SQL, SCHEMA_SQL, SCHEMA_VERSION,
};

/// The trunk thread title. The first registered session always has one.
const MAIN_THREAD_TITLE: &str = "main";

/// A SQLite-backed session store.
pub struct SqliteStore {
    conn: Mutex<Connection>,
}

impl SqliteStore {
    /// Open (or create) a store at `path`, creating the schema if absent.
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    /// Open an in-memory store (used by tests).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        // WAL keeps readers unblocked during writes. The pragma reports the
        // resulting mode as a result row, so it must be read with `query_row`;
        // an in-memory database legitimately reports `memory` instead of
        // `wal`, so the returned value is informational, not asserted.
        let _mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
        conn.pragma_update(None, "foreign_keys", true)?;
        // Refuse to proceed against an overlay written by a different
        // SCHEMA_VERSION generation. On a fresh or pre-gate v0.1.0 DB, this
        // returns `needs_stamp = true` and we mark the file current after the
        // schema steps succeed; on a match it is a no-op; on a future/foreign
        // version it returns `SchemaMismatch` and aborts open.
        let needs_stamp = Self::check_schema_version(&conn)?;
        conn.execute_batch(SCHEMA_SQL)?;
        Self::apply_additive_columns(&conn)?;
        // Stamp only after the schema steps succeeded — a stamped-but-empty
        // file would otherwise be ambiguous between "fresh and current" and
        // "current but mid-init crash".
        if needs_stamp {
            stamp_user_version(&conn, SCHEMA_VERSION)?;
        }
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// The `SCHEMA_VERSION` startup gate.
    ///
    /// Compares the binary's expected [`SCHEMA_VERSION`] against the value
    /// stored in the file's `PRAGMA user_version`, returning whether the
    /// caller still needs to stamp `user_version` after running the schema
    /// steps. Four cases:
    ///
    /// - **Fresh DB** (`user_version == 0` and the `session` table is absent).
    ///   The file has never seen delta. Returns `Ok(true)` so the caller
    ///   stamps it with [`SCHEMA_VERSION`] after `SCHEMA_SQL` creates the
    ///   tables.
    /// - **Existing v0.1.0 DB** (`user_version == 0` and the `session` table is
    ///   present). One-time rescue for users whose overlay predates the gate:
    ///   returns `Ok(true)` so the caller silently bumps `user_version` to
    ///   [`SCHEMA_VERSION`] and continues, instead of sending them to
    ///   `make reset` on first upgrade for no real reason.
    /// - **Mismatched / future DB** (`user_version != 0` and
    ///   `user_version != SCHEMA_VERSION`). The overlay was written by a
    ///   different binary generation; refuse to continue with an error naming
    ///   `make reset` as the remediation. This catches both downgrade (an
    ///   older binary against a newer DB) and a stale overlay against a newer
    ///   binary.
    /// - **Match** (`user_version == SCHEMA_VERSION`). The normal case —
    ///   returns `Ok(false)`.
    fn check_schema_version(conn: &Connection) -> Result<bool> {
        let user_version: u32 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;

        if user_version == SCHEMA_VERSION {
            return Ok(false);
        }

        if user_version == 0 {
            // Either a brand-new file (no tables yet) or an existing v0.1.0 DB
            // that predates the gate. Either way the caller stamps the file
            // current after the schema steps run. The `session` table is the
            // marker — it has shipped since v0.1.0 — but the two are not
            // distinguished here because the treatment is the same: stamp
            // current and continue. `SCHEMA_SQL`'s `IF NOT EXISTS` makes it
            // idempotent on the v0.1.0 branch.
            return Ok(true);
        }

        Err(Error::SchemaMismatch {
            found: user_version,
            expected: SCHEMA_VERSION,
        })
    }

    /// Bring an existing database up to the current column set.
    ///
    /// `CREATE TABLE IF NOT EXISTS` never alters a table that already exists, so
    /// a column added to a shipped table is invisible to a database created
    /// before it. Each [`ADDITIVE_COLUMNS`] entry is applied as a guarded
    /// `ALTER TABLE ... ADD COLUMN` only when the column is genuinely absent
    /// (SQLite has no `ADD COLUMN IF NOT EXISTS`), so this is a no-op on a fresh
    /// database that already declared the column in `SCHEMA_SQL`. Adding
    /// `session.last_activity_at` is followed by a one-time backfill so existing
    /// sessions get a correct recency value instead of a stale NULL.
    fn apply_additive_columns(conn: &Connection) -> Result<()> {
        for col in ADDITIVE_COLUMNS {
            if !column_exists(conn, col.table, col.column)? {
                conn.execute_batch(col.add_column_sql)?;
                if col.table == "session" && col.column == "last_activity_at" {
                    conn.execute_batch(BACKFILL_LAST_ACTIVITY_SQL)?;
                }
            }
        }
        // Created here rather than in `SCHEMA_SQL` because it references
        // `last_activity_at`, which an existing database only gains from the
        // guarded `ALTER TABLE` above. Idempotent (`IF NOT EXISTS`), so a fresh
        // database — where the column came from `SCHEMA_SQL` — creates it once
        // and a re-open is a no-op.
        conn.execute_batch(RECENCY_INDEX_SQL)?;
        Ok(())
    }
}

/// Write the on-disk `PRAGMA user_version` to `version`.
///
/// `PRAGMA user_version = N` does not accept bound parameters, so the value is
/// inlined; `version` is the trusted [`SCHEMA_VERSION`] constant, never user
/// input, so the format-string interpolation is safe.
fn stamp_user_version(conn: &Connection, version: u32) -> Result<()> {
    conn.execute_batch(&format!("PRAGMA user_version = {version}"))?;
    Ok(())
}

/// Whether `table` already has a column named `column`, via `PRAGMA table_info`.
fn column_exists(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    // `table` is a hard-coded schema identifier (never user input), so the
    // pragma-call form is safe; bound parameters are not accepted inside a
    // `PRAGMA table_info(...)` call.
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        if name == column {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Ensure the session's `main` thread exists, returning its id.
fn ensure_main_thread(conn: &Connection, id: &SessionId, now: &str) -> Result<ThreadId> {
    let main_id: Option<i64> = conn
        .query_row(
            "SELECT id FROM thread WHERE session_id = ?1 AND title = ?2 ORDER BY id LIMIT 1",
            params![id.as_str(), MAIN_THREAD_TITLE],
            |r| r.get(0),
        )
        .optional()?;

    let main_id = match main_id {
        Some(id) => id,
        None => {
            conn.execute(
                "INSERT INTO thread (session_id, title, parent_thread_id, created_at)
                 VALUES (?1, ?2, NULL, ?3)",
                params![id.as_str(), MAIN_THREAD_TITLE, now],
            )?;
            conn.last_insert_rowid()
        }
    };
    Ok(ThreadId(main_id))
}
