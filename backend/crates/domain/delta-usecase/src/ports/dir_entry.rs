//! One subdirectory in a directory listing.

/// A single subdirectory entry in a directory listing.
///
/// Only directories are ever listed (the picker browses folders, not files), so
/// every entry names a directory: `name` is the bare directory name and `path`
/// is its absolute path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    /// The bare directory name (the final path component).
    pub name: String,
    /// The absolute path to the directory.
    pub path: String,
}
