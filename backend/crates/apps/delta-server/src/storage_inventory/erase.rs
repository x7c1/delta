//! Deleting the files in the data directory when everything is erased: the
//! derived ones while the server still runs, then the database, the hook
//! state and the directory itself once the server has stopped and the store
//! is closed.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use super::{display, snapshots_of, StorageInventory};

impl StorageInventory {
    /// `~/.delta` when the worktree base is the default `~/.delta/worktrees`,
    /// for erasing to remove once it is empty; `None` for a base the user
    /// chose.
    pub(crate) fn worktree_base_parent(&self) -> Option<&str> {
        self.worktree_base_parent.as_deref()
    }

    /// The data directory, as reported to the browser.
    pub(crate) fn data_dir(&self) -> String {
        display(self.layout.dir())
    }

    /// Delete what the data directory holds that the store does not keep open:
    /// the per-spawn working directories, the session settings, the tmux
    /// configuration and the migration snapshots.
    ///
    /// Run after every session is closed and Delta's tmux server is gone, so
    /// nothing is still running in a working directory or about to read its
    /// settings. A failure is logged at `warn` and the rest are still deleted:
    /// the data directory then stays at the end, with what could not go.
    pub(crate) fn delete_derived_files(&self) {
        remove_tree(&self.layout.sessions());
        remove_tree(&self.layout.settings_dir());
        remove_file(&self.layout.tmux_conf());
        for snapshot in snapshots_of(&self.layout.database()) {
            remove_file(Path::new(&snapshot.path));
        }
    }

    /// Delete the database with the files SQLite keeps beside it, the hook
    /// state file, and then the data directory itself, returning whether the
    /// directory is gone.
    ///
    /// **Only once the store is closed**: SQLite recreates its `-wal` and
    /// `-shm` files by name while a connection is open, so deleting them under
    /// an open store could leave them behind. The directory is removed only
    /// when nothing else is left in it — an explicit `DELTA_DATA_DIR` may name
    /// a directory that also holds the user's own files.
    pub(crate) fn delete_data_dir(&self) -> bool {
        let database = self.layout.database();
        for path in [
            sibling(&database, "-wal"),
            sibling(&database, "-shm"),
            database,
            self.layout.hook_state(),
        ] {
            remove_file(&path);
        }
        let dir = self.layout.dir();
        match std::fs::remove_dir(dir) {
            Ok(()) => {
                tracing::info!(dir = %dir.display(), "deleted the data directory");
                true
            }
            Err(err) if err.kind() == ErrorKind::NotFound => true,
            Err(err) => {
                tracing::warn!(
                    dir = %dir.display(),
                    error = %err,
                    "kept the data directory: it still holds files Delta did not create, \
                     or could not be removed"
                );
                false
            }
        }
    }
}

/// `path` with `suffix` appended to its file name (`delta.db` → `delta.db-wal`).
fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Delete the file at `path`; a missing one is already gone.
fn remove_file(path: &Path) {
    match std::fs::remove_file(path) {
        Ok(()) => tracing::info!(path = %path.display(), "deleted"),
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        Err(err) => tracing::warn!(path = %path.display(), error = %err, "could not delete"),
    }
}

/// Delete the directory at `path` with everything in it; a missing one is
/// already gone.
fn remove_tree(path: &Path) {
    match std::fs::remove_dir_all(path) {
        Ok(()) => tracing::info!(path = %path.display(), "deleted"),
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        Err(err) => tracing::warn!(path = %path.display(), error = %err, "could not delete"),
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::inventory;

    /// Planted in a data directory the way a running server leaves it.
    const DERIVED: [&str; 4] = [
        "sessions/tok/notes.txt",
        "settings/7878.json",
        "tmux.conf",
        "delta.db.bak-v3",
    ];
    const HELD: [&str; 4] = [
        "delta.db",
        "delta.db-wal",
        "delta.db-shm",
        "delta-hook-state.json",
    ];

    fn plant(root: &std::path::Path, names: &[&str]) {
        for name in names {
            let path = root.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"x").unwrap();
        }
    }

    #[test]
    fn the_derived_files_go_first_and_the_rest_with_the_directory() {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("data");
        plant(&data, &DERIVED);
        plant(&data, &HELD);
        let inventory = inventory(&data);

        inventory.delete_derived_files();

        for name in ["sessions", "settings", "tmux.conf", "delta.db.bak-v3"] {
            assert!(!data.join(name).exists(), "{name} is gone");
        }
        for name in HELD {
            assert!(
                data.join(name).exists(),
                "{name} waits for the store to close"
            );
        }

        assert!(inventory.delete_data_dir());
        assert!(!data.exists());
        assert!(inventory.delete_data_dir(), "a second call finds it gone");
    }

    #[test]
    fn a_data_directory_holding_other_files_is_kept_with_them() {
        let root = tempfile::tempdir().unwrap();
        plant(root.path(), &HELD);
        plant(root.path(), &["mine.txt"]);
        let inventory = inventory(root.path());

        assert!(!inventory.delete_data_dir());

        assert!(root.path().join("mine.txt").exists());
        for name in HELD {
            assert!(!root.path().join(name).exists(), "{name} is gone");
        }
    }
}
