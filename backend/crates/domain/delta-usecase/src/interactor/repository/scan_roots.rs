//! CRUD use cases for repository scan roots.

use crate::error::Result;
use crate::interactor::InteractorCore;
use crate::ports::{
    GitWorktree, RepositoryScanRoot, SessionStore, TmuxDriver, Transcript, Workspace,
};

impl<T, X, S, W, G> InteractorCore<T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// The user's registered repository scan roots, newest first.
    pub async fn list_repository_scan_roots(&self) -> Result<Vec<RepositoryScanRoot>> {
        self.store.list_repository_scan_roots().await
    }

    /// Register a new repository scan root. The path is taken verbatim — the
    /// CRUD endpoint trims and validates absolute/non-empty before calling.
    pub async fn add_repository_scan_root(&self, path: &str) -> Result<RepositoryScanRoot> {
        self.store.insert_repository_scan_root(path).await
    }

    /// Unregister a repository scan root. Idempotent.
    pub async fn remove_repository_scan_root(&self, path: &str) -> Result<()> {
        self.store.delete_repository_scan_root(path).await
    }
}
