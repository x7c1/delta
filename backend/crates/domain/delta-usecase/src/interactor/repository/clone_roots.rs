//! CRUD use cases for clone roots.

use crate::error::Result;
use crate::interactor::InteractorCore;
use crate::ports::{CloneRoot, GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};

impl<T, X, S, W, G> InteractorCore<T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// The user's registered clone roots, newest first.
    pub async fn list_clone_roots(&self) -> Result<Vec<CloneRoot>> {
        self.store.list_clone_roots().await
    }

    /// Register a new clone root. The path is taken verbatim — the CRUD
    /// endpoint trims and validates absolute/non-empty before calling.
    pub async fn add_clone_root(&self, path: &str) -> Result<CloneRoot> {
        self.store.insert_clone_root(path).await
    }

    /// Unregister a clone root. Idempotent.
    pub async fn remove_clone_root(&self, path: &str) -> Result<()> {
        self.store.delete_clone_root(path).await
    }
}
