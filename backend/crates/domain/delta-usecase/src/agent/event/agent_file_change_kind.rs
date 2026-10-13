//! How one file would change if a permission request is allowed.

#[cfg(doc)]
use super::AgentFileChange;

/// How one file would change if a permission request is allowed.
///
/// Deliberately a small, closed vocabulary rather than a passthrough of any one
/// provider's spelling: these three are what a patch can do to a path, and a
/// request that names something else degrades to no kind at all (see
/// [`AgentFileChange::kind`]) instead of inventing one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentFileChangeKind {
    /// The file does not exist yet and would be created.
    Add,
    /// An existing file's contents would be edited.
    Update,
    /// The file would be removed.
    Delete,
}
