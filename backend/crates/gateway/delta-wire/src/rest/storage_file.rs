//! One file Delta keeps on disk, with its size, for `GET /api/storage`.

use serde::Serialize;
use ts_rs::TS;

/// A file in Delta's storage inventory: its absolute path and its size in
/// bytes, read when the request was served.
///
/// `bytes` may sum several files the user thinks of as one — the database is
/// `delta.db` plus SQLite's `-wal` and `-shm` beside it, reported at
/// `delta.db`'s path. A file that does not exist counts as zero bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "StorageFile")]
pub struct WireStorageFile {
    pub path: String,
    pub bytes: u64,
}
