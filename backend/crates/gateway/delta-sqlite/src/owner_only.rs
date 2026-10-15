//! Keeping the database and its snapshots readable by their owner alone.
//!
//! The database holds every prompt and reply Delta has seen, so it, the
//! `-wal`/`-shm` files SQLite keeps beside it and the pre-migration snapshots
//! are made `0600`. SQLite creates a file with the process's default mode
//! (`0644` under a typical umask), and only creates the `-wal`/`-shm` files
//! with the database file's own mode, so the database file is created `0600`
//! before SQLite opens it, and every file is set to `0600` after: that also
//! tightens files an older Delta left readable.

use std::fs::{OpenOptions, Permissions};
use std::io::{self, ErrorKind};
use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// The mode of every file this module touches.
const OWNER_ONLY_FILE_MODE: u32 = 0o600;

/// The suffixes SQLite gives the files it keeps beside a WAL-mode database.
const SIDE_FILE_SUFFIXES: [&str; 2] = ["-wal", "-shm"];

/// The infix of a pre-migration snapshot's name (`<db>.bak-v<N>`).
const SNAPSHOT_INFIX: &str = ".bak-v";

/// Create the database file at `path` empty and owner-only when it does not
/// exist yet (SQLite takes an empty file for a new database), so it is never
/// readable by others even for a moment.
pub(crate) fn create_database_file(path: &Path) -> Result<()> {
    let created = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(OWNER_ONLY_FILE_MODE)
        .open(path);
    match created {
        Ok(_) => Ok(()),
        Err(err) if err.kind() == ErrorKind::AlreadyExists => Ok(()),
        Err(source) => Err(mode_error(path, source)),
    }
}

/// Make the database at `path`, the `-wal`/`-shm` files beside it and its
/// pre-migration snapshots owner-only, whichever of them exist.
pub(crate) fn restrict_database_files(path: &Path) -> Result<()> {
    restrict(path)?;
    for suffix in SIDE_FILE_SUFFIXES {
        restrict(&with_suffix(path, suffix))?;
    }
    for snapshot in snapshots_of(path)? {
        restrict(&snapshot)?;
    }
    Ok(())
}

/// Set the file at `path` to [`OWNER_ONLY_FILE_MODE`] when it exists with any
/// other mode.
pub(crate) fn restrict(path: &Path) -> Result<()> {
    let meta = match std::fs::metadata(path) {
        Ok(meta) => meta,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(()),
        Err(source) => return Err(mode_error(path, source)),
    };
    if meta.permissions().mode() & 0o777 == OWNER_ONLY_FILE_MODE {
        return Ok(());
    }
    std::fs::set_permissions(path, Permissions::from_mode(OWNER_ONLY_FILE_MODE))
        .map_err(|source| mode_error(path, source))
}

/// The pre-migration snapshots (`<db>.bak-v<N>`) beside the database at
/// `path`.
fn snapshots_of(path: &Path) -> Result<Vec<PathBuf>> {
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
        return Ok(Vec::new());
    };
    let dir = if dir.as_os_str().is_empty() {
        Path::new(".")
    } else {
        dir
    };
    let prefix = format!("{}{SNAPSHOT_INFIX}", name.to_string_lossy());
    let entries = std::fs::read_dir(dir).map_err(|source| mode_error(dir, source))?;
    let mut snapshots = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| mode_error(dir, source))?;
        if entry.file_name().to_string_lossy().starts_with(&prefix) {
            snapshots.push(entry.path());
        }
    }
    Ok(snapshots)
}

/// `path` with `suffix` appended to its file name.
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

fn mode_error(path: &Path, source: io::Error) -> Error {
    Error::FileMode {
        path: path.display().to_string(),
        source,
    }
}
