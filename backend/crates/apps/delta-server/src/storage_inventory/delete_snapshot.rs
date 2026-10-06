//! [`StorageInventory::delete_snapshot`]: deleting one listed migration
//! snapshot for `DELETE /api/storage/snapshots`.

use super::{snapshots_of, StorageInventory};

/// Why [`StorageInventory::delete_snapshot`] deleted nothing.
#[derive(Debug, thiserror::Error)]
pub(crate) enum SnapshotDeletionError {
    /// The path is not one of the snapshots the inventory lists. Only those
    /// are deletable: the check is what keeps the route from deleting any
    /// other file the server can reach.
    #[error("not a listed migration snapshot: {0}")]
    NotListed(String),
    /// The snapshot is listed but deleting it failed.
    #[error("could not delete the migration snapshot {path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
}

impl StorageInventory {
    /// Delete the migration snapshot at `path`, which must be one of the
    /// snapshots [`Self::report`] lists right now, spelled exactly as listed.
    ///
    /// The runner keeps these until the user deletes them here: each is a
    /// full copy of the database taken before a migration that rewrote it,
    /// so it is the user's to decide when it is no longer needed.
    pub(crate) fn delete_snapshot(&self, path: &str) -> Result<(), SnapshotDeletionError> {
        let listed = snapshots_of(&self.layout.database())
            .into_iter()
            .any(|snapshot| snapshot.path == path);
        if !listed {
            return Err(SnapshotDeletionError::NotListed(path.to_owned()));
        }
        std::fs::remove_file(path).map_err(|source| SnapshotDeletionError::Io {
            path: path.to_owned(),
            source,
        })?;
        tracing::info!(path, "deleted a migration snapshot");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::display;
    use super::super::testing::inventory;
    use super::*;

    #[test]
    fn a_listed_snapshot_is_deleted_and_nothing_else_is() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "delta.db",
            "delta.db.bak-v2",
            "delta.db.bak-vx",
            "other.txt",
        ] {
            std::fs::write(dir.path().join(name), b"x").unwrap();
        }
        let inventory = inventory(dir.path());
        let path = |name: &str| display(&dir.path().join(name));

        inventory.delete_snapshot(&path("delta.db.bak-v2")).unwrap();

        assert!(!dir.path().join("delta.db.bak-v2").exists());
        assert!(inventory.report("sock", "v0".into()).snapshots.is_empty());
        for name in [
            "delta.db",
            "delta.db.bak-vx",
            "other.txt",
            "delta.db.bak-v2",
        ] {
            let refused = inventory.delete_snapshot(&path(name));
            assert!(
                matches!(refused, Err(SnapshotDeletionError::NotListed(_))),
                "{name} is not a listed snapshot, got {refused:?}"
            );
        }
        assert!(dir.path().join("delta.db").exists());
        assert!(dir.path().join("other.txt").exists());
        let traversal = format!("{}/../delta.db.bak-v2", dir.path().display());
        assert!(matches!(
            inventory.delete_snapshot(&traversal),
            Err(SnapshotDeletionError::NotListed(_))
        ));
    }
}
