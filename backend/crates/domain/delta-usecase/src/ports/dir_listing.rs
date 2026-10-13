//! Read-only directory-browsing data the [`crate::Workspace`] port returns.

use super::DirEntry;

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
    /// The immediate subdirectories, sorted by name (case-insensitive).
    /// Dot-directories are excluded unless the listing was requested with
    /// `include_hidden`.
    pub entries: Vec<DirEntry>,
}
