//! [`StorageInventory::report`]: the inventory as `GET /api/storage` returns
//! it, with sizes read from disk on request.

use std::path::{Path, PathBuf};

use delta_wire::rest::{WireStorageFile, WireStorageResponse};

use super::{display, file_bytes, snapshots_of, StorageInventory};

/// The suffixes SQLite gives the files it keeps beside the database in WAL
/// mode. Counted into the database's size: a user sees one number for "the
/// database".
const SQLITE_SIDECAR_SUFFIXES: [&str; 2] = ["-wal", "-shm"];

impl StorageInventory {
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

/// `path` with `suffix` appended to its file name (`delta.db` → `delta.db-wal`).
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::super::testing::inventory;
    use super::*;

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
