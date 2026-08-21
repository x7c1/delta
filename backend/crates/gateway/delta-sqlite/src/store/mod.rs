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
use crate::migrations::{self, SCHEMA_VERSION};

/// The trunk thread title. The first registered session always has one.
const MAIN_THREAD_TITLE: &str = "main";

/// A SQLite-backed session store.
pub struct SqliteStore {
    conn: Mutex<Connection>,
}

impl SqliteStore {
    /// Open (or create) a store at `path`, bringing its schema up to date.
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        Self::init(conn, Some(path))
    }

    /// Open an in-memory store (used by tests). Its schema is built by replaying
    /// the same ladder a file-backed database is, so every test runs against the
    /// real thing.
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::init(conn, None)
    }

    /// `db_path` is the file the connection was opened from, or `None` for an
    /// in-memory database; the migration runner needs it to name a
    /// pre-migration snapshot.
    fn init(conn: Connection, db_path: Option<&str>) -> Result<Self> {
        // WAL keeps readers unblocked during writes. The pragma reports the
        // resulting mode as a result row, so it must be read with `query_row`;
        // an in-memory database legitimately reports `memory` instead of
        // `wal`, so the returned value is informational, not asserted.
        let _mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
        conn.pragma_update(None, "foreign_keys", true)?;
        Self::migrate_to_current(&conn, db_path)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// The startup schema gate: bring the file to [`SCHEMA_VERSION`], or refuse
    /// to open it.
    ///
    /// Six cases, keyed on the file's `PRAGMA user_version`:
    ///
    /// - **Current** (`== SCHEMA_VERSION`). Nothing to do: no step is applied,
    ///   no backup is written, and the stamp is not rewritten.
    /// - **Newer** (`> SCHEMA_VERSION`). Written by a newer binary. The ladder
    ///   only runs forward, so this stays a hard refusal
    ///   ([`Error::SchemaMismatch`]).
    /// - **Fresh** (`== 0`, no `session` table). A file that has never seen
    ///   delta: the whole ladder is replayed from 0, which is what builds the
    ///   schema.
    /// - **Pre-gate v0.1.0 overlay** (`== 0`, `session` table present). Delta's
    ///   tables are there but nothing recorded which generation wrote them, so
    ///   the file's real shape is unknown and the baseline cannot be safely
    ///   replayed onto it. Refused ([`Error::UnstampedOverlay`]) with a
    ///   `make reset` hint. Such a database predates the gate entirely and
    ///   cannot still be in circulation.
    /// - **Older than the ladder** (`0 < v <` the baseline). A delta generation
    ///   the ladder squashed rather than reconstructed — v0.2.x and v0.3.0 both
    ///   stamped 1, and nothing on the ladder carries a 1 or a 2 forward.
    ///   Refused ([`Error::PreBaselineOverlay`]), because replaying the baseline
    ///   over it would no-op (every statement is `IF NOT EXISTS`) and then stamp
    ///   the older shape as current.
    /// - **Behind** (baseline `<= v < SCHEMA_VERSION`). The pending steps are
    ///   applied.
    ///
    /// The ladder itself is [`validate`](crate::migrations::validate)d on every
    /// open, before any of that, so an inconsistent registry fails loudly here
    /// instead of silently skipping a step.
    fn migrate_to_current(conn: &Connection, db_path: Option<&str>) -> Result<()> {
        let steps = migrations::registry();
        migrations::validate(&steps, SCHEMA_VERSION)?;

        let user_version: u32 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if user_version > SCHEMA_VERSION {
            return Err(Error::SchemaMismatch {
                found: user_version,
                expected: SCHEMA_VERSION,
            });
        }
        if user_version == 0 && has_session_table(conn)? {
            return Err(Error::UnstampedOverlay);
        }
        // `validate` has already established that the ladder is non-empty and
        // ascending, so its first step is the oldest version it can start from.
        let baseline = steps[0].to_version;
        if user_version != 0 && user_version < baseline {
            return Err(Error::PreBaselineOverlay {
                found: user_version,
                baseline,
            });
        }
        migrations::migrate(conn, &steps, user_version, SCHEMA_VERSION, db_path)
    }
}

/// Whether the database already has delta's `session` table.
///
/// The marker that separates a brand-new file from a pre-gate v0.1.0 overlay:
/// both carry `user_version = 0`, and `session` has shipped since v0.1.0.
fn has_session_table(conn: &Connection) -> Result<bool> {
    let found: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'session'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    Ok(found.is_some())
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
