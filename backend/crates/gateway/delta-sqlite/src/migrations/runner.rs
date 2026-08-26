//! The ladder runner: applies pending steps to whatever version a database is
//! at, one transaction per version.

use std::path::Path;

use rusqlite::Connection;
use tracing::info;

use super::{Step, StepKind};
use crate::error::Result;

/// Apply every step that takes `from_version` up to `target_version`.
///
/// `steps` is the ladder — the production path passes
/// [`super::registry`], tests pass synthetic ladders. It must already be
/// ordered the way the steps are to be applied: ascending by
/// [`Step::to_version`], and, within one version, in the order the statements
/// depend on each other (a table before its indexes and triggers).
///
/// The pending set is every step whose `to_version` is greater than
/// `from_version` and not greater than `target_version`. Nothing pending is a
/// no-op: no transaction is opened, no backup is written, and `user_version` is
/// left exactly as it was — which is what an already-current database must see.
///
/// **One transaction per version.** Every step of a version is applied inside a
/// single transaction that also stamps `PRAGMA user_version` (which SQLite keeps
/// in the database header, and therefore rolls back with everything else). So a
/// version either lands whole and is stamped, or does not land at all; an upgrade
/// interrupted or failed part-way through a multi-version ladder resumes from the
/// last version that fully landed instead of replaying a partial one.
///
/// **Backup.** If the pending set contains at least one destructive step, a
/// snapshot of the database is taken *before* anything is applied — see
/// [`back_up`]. An additive-only upgrade writes no file, and neither does a
/// replay from 0, which is building a fresh database rather than changing one.
/// `db_path` is the file the connection was opened from, or `None` for an
/// in-memory database (nothing on disk to snapshot).
pub(crate) fn migrate(
    conn: &Connection,
    steps: &[Step],
    from_version: u32,
    target_version: u32,
    db_path: Option<&str>,
) -> Result<()> {
    let pending: Vec<&Step> = steps
        .iter()
        .filter(|step| step.to_version > from_version && step.to_version <= target_version)
        .collect();
    if pending.is_empty() {
        return Ok(());
    }

    if pending
        .iter()
        .any(|step| step.kind == StepKind::Destructive)
    {
        back_up(conn, db_path, from_version)?;
    }

    for version_steps in pending.chunk_by(|a, b| a.to_version == b.to_version) {
        // Every step in the chunk shares a `to_version`, so the first one names
        // the version this transaction produces.
        let version = version_steps[0].to_version;
        let tx = conn.unchecked_transaction()?;
        for step in version_steps {
            tx.execute_batch(step.sql)?;
        }
        // `PRAGMA user_version = N` does not accept bound parameters, so the
        // value is inlined; it is a `u32` from a compiled-in step, never user
        // input. Stamped inside the transaction so the mark and the change it
        // describes commit together.
        tx.execute_batch(&format!("PRAGMA user_version = {version}"))?;
        tx.commit()?;
        info!(
            schema_version = version,
            steps = version_steps.len(),
            "applied delta SQLite schema migration"
        );
    }
    Ok(())
}

/// Snapshot the database next to itself as `<db_path>.bak-v<from_version>`,
/// before a destructive pending set is applied.
///
/// The name carries the *source* version, so a given database writes each such
/// file exactly once in its life however many times it is opened.
///
/// **A replay from version 0 is skipped.** Such a run is not an upgrade: the
/// file has never seen delta (the gate refuses an unstamped file that already
/// holds delta's tables), so the whole ladder — the destructive steps included
/// — is being replayed to *build* the schema, over nothing that could be lost.
/// Without this, every fresh install would find a `delta.db.bak-v0` snapshot of
/// an empty database sitting next to its brand-new one, and no way to tell it
/// from a real pre-upgrade copy.
///
/// `VACUUM INTO` rather than a file copy: the database runs in WAL mode, so a
/// plain copy can miss changes that have not been checkpointed back into the
/// main file, while `VACUUM INTO` writes a consistent single-file snapshot
/// through the same connection.
///
/// **An existing target file is left alone and the migration proceeds.** SQLite
/// refuses `VACUUM INTO` onto an existing path, and a retry after a failed
/// migration would otherwise be unable to start at all; the file that is already
/// there is the correct pre-migration snapshot, because the failed attempt rolled
/// back. Backups are never deleted automatically — their main value is the
/// migration that appeared to succeed and is found to be wrong days later, which
/// is exactly when an auto-cleanup would have removed the only copy. The one
/// thing that does remove them is `scripts/dev.sh --reset` (`make reset`), which
/// deletes them together with the database they were taken from — leaving them
/// behind would make them look like snapshots of the database the reset created.
fn back_up(conn: &Connection, db_path: Option<&str>, from_version: u32) -> Result<()> {
    if from_version == 0 {
        info!("skipping pre-migration backup: replaying the ladder onto a fresh database");
        return Ok(());
    }
    let Some(db_path) = db_path else {
        // An in-memory database has no file to snapshot, and `VACUUM INTO` would
        // happily write one anyway — a surprise nobody asked for. Say so and
        // carry on: the data is process-local and disappears at exit regardless.
        info!("skipping pre-migration backup: the database is in-memory");
        return Ok(());
    };
    let backup_path = format!("{db_path}.bak-v{from_version}");
    if Path::new(&backup_path).exists() {
        info!(
            path = %backup_path,
            "pre-migration backup already exists, keeping it and continuing"
        );
        return Ok(());
    }
    // `VACUUM INTO` takes a string literal, not a bound parameter, so the path is
    // inlined with SQL's own escaping (a single quote doubled).
    let quoted = backup_path.replace('\'', "''");
    conn.execute_batch(&format!("VACUUM INTO '{quoted}'"))?;
    info!(
        path = %backup_path,
        "wrote pre-migration backup before applying a destructive schema migration"
    );
    Ok(())
}
