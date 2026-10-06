//! [`StorageInventory`]: where this running Delta keeps its files, measured on
//! request for `GET /api/storage`.
//!
//! Split by method: this module holds the struct, its constructors and the
//! helpers both methods share, `report` the inventory itself, and
//! `delete_snapshot` the deletion of a listed migration snapshot.

mod delete_snapshot;
pub(crate) use delete_snapshot::SnapshotDeletionError;
mod report;
#[cfg(test)]
mod testing;

use std::path::{Path, PathBuf};

use delta_bootstrap::{Config, DataLayout};
use delta_wire::rest::WireStorageFile;

/// The infix the migration runner puts between the database's file name and
/// the schema version it snapshots (`delta.db.bak-v<N>`).
const SNAPSHOT_INFIX: &str = ".bak-v";

/// The paths a running server writes, fixed at startup, from which
/// [`Self::report`] builds the storage inventory.
///
/// Holds paths only — never the hook secret or the auth token — and no sizes:
/// those are read from disk on every [`Self::report`], so a reopened Settings
/// category shows the database as it is now.
#[derive(Debug, Clone)]
pub struct StorageInventory {
    identifier: String,
    layout: DataLayout,
    port: u16,
    worktree_base: PathBuf,
    transcript_root: PathBuf,
}

impl StorageInventory {
    /// The inventory of a server running as `identifier` on `port`, writing
    /// under `data_dir`, with worktrees under `worktree_base` and transcripts
    /// read from under `transcript_root`.
    pub fn new(
        identifier: impl Into<String>,
        data_dir: impl Into<PathBuf>,
        port: u16,
        worktree_base: impl Into<PathBuf>,
        transcript_root: impl Into<PathBuf>,
    ) -> Self {
        Self {
            identifier: identifier.into(),
            layout: DataLayout::new(data_dir),
            port,
            worktree_base: worktree_base.into(),
            transcript_root: transcript_root.into(),
        }
    }

    /// The inventory of the server `config` configures.
    pub fn from_config(config: &Config) -> Self {
        Self {
            identifier: config.identifier.clone(),
            layout: config.data_layout(),
            port: config.port,
            worktree_base: PathBuf::from(&config.worktree_base),
            transcript_root: PathBuf::from(&config.transcript_root),
        }
    }
}

/// The migration runner's `<database>.bak-v<N>` snapshots beside `database`,
/// in ascending `<N>`, each with its size.
///
/// An unreadable directory yields no snapshots, with a warning unless the
/// directory does not exist (a data directory not created yet has none).
fn snapshots_of(database: &Path) -> Vec<WireStorageFile> {
    let (Some(dir), Some(name)) = (database.parent(), database.file_name()) else {
        return Vec::new();
    };
    let prefix = format!("{}{SNAPSHOT_INFIX}", name.to_string_lossy());
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) => {
            if err.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(dir = %dir.display(), error = %err, "could not list the database snapshots");
            }
            return Vec::new();
        }
    };
    let mut snapshots: Vec<(u64, PathBuf)> = entries
        .filter_map(|entry| match entry {
            Ok(entry) => Some(entry.path()),
            Err(err) => {
                tracing::warn!(dir = %dir.display(), error = %err, "could not read a directory entry while listing the database snapshots");
                None
            }
        })
        .filter_map(|path| {
            let version = path
                .file_name()?
                .to_str()?
                .strip_prefix(&prefix)?
                .parse::<u64>()
                .ok()?;
            Some((version, path))
        })
        .collect();
    snapshots.sort();
    snapshots
        .into_iter()
        .map(|(_, path)| WireStorageFile {
            bytes: file_bytes(&path),
            path: display(&path),
        })
        .collect()
}

/// The size of the file at `path`, or zero when it cannot be read — silently
/// when it does not exist, with a warning otherwise.
fn file_bytes(path: &Path) -> u64 {
    match std::fs::metadata(path) {
        Ok(metadata) => metadata.len(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => 0,
        Err(err) => {
            tracing::warn!(path = %path.display(), error = %err, "could not read a file size for the storage inventory");
            0
        }
    }
}

fn display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
