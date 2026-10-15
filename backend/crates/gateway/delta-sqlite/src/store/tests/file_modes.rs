//! The database's files are readable by their owner alone, whatever the umask
//! and whatever mode an older Delta left them with.

use std::fs::Permissions;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use super::super::SqliteStore;
use super::new_session;

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    PathBuf::from(format!("{}{suffix}", path.display()))
}

#[tokio::test]
async fn an_opened_database_and_its_wal_files_are_owner_only() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("delta.db");

    let store = SqliteStore::open(path.to_str().unwrap()).unwrap();
    store.register_session(new_session()).await.unwrap();

    for file in [path.clone(), sibling(&path, "-wal"), sibling(&path, "-shm")] {
        assert!(file.exists(), "{} is there", file.display());
        assert_eq!(mode(&file), 0o600, "{}", file.display());
    }
}

#[tokio::test]
async fn opening_tightens_files_left_readable_by_others() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("delta.db");
    let snapshot = sibling(&path, ".bak-v3");
    drop(SqliteStore::open(path.to_str().unwrap()).unwrap());
    std::fs::write(&snapshot, b"").unwrap();
    for file in [&path, &snapshot] {
        std::fs::set_permissions(file, Permissions::from_mode(0o644)).unwrap();
    }

    let _store = SqliteStore::open(path.to_str().unwrap()).unwrap();

    for file in [
        path.clone(),
        sibling(&path, "-wal"),
        sibling(&path, "-shm"),
        snapshot,
    ] {
        assert_eq!(mode(&file), 0o600, "{}", file.display());
    }
}
