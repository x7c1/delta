//! [`StorageInventory`]: where this running Delta keeps its files, measured on
//! request for `GET /api/storage`.

use std::path::{Path, PathBuf};

use delta_bootstrap::{Config, DataLayout};
use delta_wire::rest::{WireStorageFile, WireStorageResponse};

/// The infix the migration runner puts between the database's file name and
/// the schema version it snapshots (`delta.db.bak-v<N>`).
const SNAPSHOT_INFIX: &str = ".bak-v";

/// The suffixes SQLite gives the files it keeps beside the database in WAL
/// mode. Counted into the database's size: a user sees one number for "the
/// database".
const SQLITE_SIDECAR_SUFFIXES: [&str; 2] = ["-wal", "-shm"];

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

    /// The inventory as the browser sees it, with sizes read from disk now.
    ///
    /// `tmux_socket` and `version` are passed in because the server already
    /// holds each in one place ([`crate::AppState::tmux_socket`],
    /// [`crate::display_version`]). A size that cannot be read counts as zero:
    /// a missing file silently (the `-wal`/`-shm` files come and go), any
    /// other failure with a warning.
    ///
    /// Blocking file-system calls, but only a handful of `stat`s and one
    /// directory listing, so the handler runs it inline.
    pub(crate) fn report(&self, tmux_socket: &str, version: String) -> WireStorageResponse {
        let database = self.layout.database();
        let database_bytes = file_bytes(&database)
            + SQLITE_SIDECAR_SUFFIXES
                .iter()
                .map(|suffix| file_bytes(&with_suffix(&database, suffix)))
                .sum::<u64>();
        WireStorageResponse {
            identifier: self.identifier.clone(),
            version,
            data_dir: display(self.layout.dir()),
            database: WireStorageFile {
                path: display(&database),
                bytes: database_bytes,
            },
            snapshots: snapshots_of(&database),
            hook_state: display(&self.layout.hook_state()),
            sessions_dir: display(&self.layout.sessions()),
            session_settings: display(&self.layout.session_settings(self.port)),
            tmux_conf: display(&self.layout.tmux_conf()),
            tmux_socket: tmux_socket.to_owned(),
            worktree_base: display(&self.worktree_base),
            transcript_root: display(&self.transcript_root),
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

/// `path` with `suffix` appended to its file name (`delta.db` → `delta.db-wal`).
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn inventory(data_dir: &Path) -> StorageInventory {
        StorageInventory::new("delta-test", data_dir, 7878, "/w", "/t")
    }

    #[test]
    fn the_database_size_sums_the_database_and_its_sqlite_sidecars() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("delta.db"), vec![0; 100]).unwrap();
        std::fs::write(dir.path().join("delta.db-wal"), vec![0; 20]).unwrap();
        // No `-shm`: a missing sidecar counts as zero.
        let report = inventory(dir.path()).report("sock", "v0".into());
        assert_eq!(report.database.bytes, 120);
        assert_eq!(report.database.path, display(&dir.path().join("delta.db")));
    }

    #[test]
    fn a_missing_data_dir_reports_zero_and_no_snapshots() {
        let dir = tempfile::tempdir().unwrap();
        let report = inventory(&dir.path().join("absent")).report("sock", "v0".into());
        assert_eq!(report.database.bytes, 0);
        assert!(report.snapshots.is_empty());
    }

    #[test]
    fn snapshots_are_listed_in_version_order_and_other_files_are_not() {
        let dir = tempfile::tempdir().unwrap();
        for (name, len) in [
            ("delta.db.bak-v10", 3),
            ("delta.db.bak-v2", 5),
            ("delta.db.bak-vx", 1),
            ("other.db.bak-v1", 1),
            ("delta.db", 1),
        ] {
            std::fs::write(dir.path().join(name), vec![0; len]).unwrap();
        }
        let report = inventory(dir.path()).report("sock", "v0".into());
        let listed: Vec<_> = report
            .snapshots
            .iter()
            .map(|file| {
                (
                    Path::new(&file.path).file_name().unwrap().to_owned(),
                    file.bytes,
                )
            })
            .collect();
        assert_eq!(
            listed,
            [
                ("delta.db.bak-v2".into(), 5),
                ("delta.db.bak-v10".into(), 3)
            ],
        );
    }

    #[test]
    fn the_report_carries_every_derived_path_and_the_passed_in_values() {
        let report = inventory(Path::new("/d")).report("sock", "v1.2.3".into());
        assert_eq!(report.identifier, "delta-test");
        assert_eq!(report.version, "v1.2.3");
        assert_eq!(report.data_dir, "/d");
        assert_eq!(report.hook_state, "/d/delta-hook-state.json");
        assert_eq!(report.sessions_dir, "/d/sessions");
        assert_eq!(report.session_settings, "/d/settings/7878.json");
        assert_eq!(report.tmux_conf, "/d/tmux.conf");
        assert_eq!(report.tmux_socket, "sock");
        assert_eq!(report.worktree_base, "/w");
        assert_eq!(report.transcript_root, "/t");
    }
}
