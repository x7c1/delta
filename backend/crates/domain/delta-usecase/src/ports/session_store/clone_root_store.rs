//! The clone roots registered in settings.

use async_trait::async_trait;

use super::clone_root::CloneRoot;
use super::SessionStore;
use crate::error::Result;

/// The registered clone roots: the directories the Repository tab probes for
/// git clones.
///
/// One of the concerns [`SessionStore`] gathers.
#[async_trait]
pub trait CloneRootStore: Send + Sync {
    /// The registered clone roots, most-recently-added first.
    ///
    /// Each row is a directory whose direct children the Repository tab will
    /// probe for git clones on every list call. The set is small (one entry per
    /// directory the user has registered) so the whole list is returned at once.
    async fn list_clone_roots(&self) -> Result<Vec<CloneRoot>>;

    /// Register a new clone root. Returns the created row, or
    /// [`crate::Error::CloneRootDuplicate`] when `path` is already registered —
    /// the PRIMARY KEY constraint is the conflict gate, so callers do not need a
    /// pre-check.
    async fn insert_clone_root(&self, path: &str) -> Result<CloneRoot>;

    /// Unregister a clone root. Deleting an unknown path is a no-op (idempotent),
    /// so the Settings dialog's explicit Remove click never surfaces a 404 noise
    /// on a path the user just removed via another tab.
    async fn delete_clone_root(&self, path: &str) -> Result<()>;
}

#[async_trait]
impl CloneRootStore for Box<dyn SessionStore> {
    async fn list_clone_roots(&self) -> Result<Vec<CloneRoot>> {
        (**self).list_clone_roots().await
    }

    async fn insert_clone_root(&self, path: &str) -> Result<CloneRoot> {
        (**self).insert_clone_root(path).await
    }

    async fn delete_clone_root(&self, path: &str) -> Result<()> {
        (**self).delete_clone_root(path).await
    }
}
