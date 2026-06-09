//! Read-only directory-browsing data the [`crate::Workspace`] port returns.

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

/// One level of a directory browse: a canonical directory, its parent, and its
/// immediate subdirectories.
///
/// Returned by [`crate::Workspace::list_dirs`] so a directory-picker UI can show
/// where it is (`path`), step up (`parent`), and step into a child (`entries`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirListing {
    /// The canonical absolute path that was listed.
    pub path: String,
    /// The canonical absolute path of the parent directory, or `None` when
    /// `path` is a filesystem root (there is nowhere further up).
    pub parent: Option<String>,
    /// The immediate subdirectories, sorted by name (case-insensitive),
    /// dot-directories excluded.
    pub entries: Vec<DirEntry>,
}
