use crate::error::Result;
use crate::interactor::InteractorCore;
use crate::ports::{DirListing, GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};

use super::home_dir;

impl<T, X, S, W, G> InteractorCore<T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Browse the immediate subdirectories of `path` for the directory picker.
    ///
    /// Delegates to [`Workspace::list_dirs`], which returns the canonical path,
    /// its parent, and the immediate subdirectories (dirs only, dot-directories
    /// excluded, sorted by name). A `None` or empty `path` defaults to the user's
    /// home directory so the picker has a sensible starting point. A missing
    /// path, a non-directory, or a permission error surfaces as a clean
    /// `InvalidWorkdir`/`WorkdirPermission` rather than an internal failure.
    pub async fn browse_workdir(&self, path: Option<&str>) -> Result<DirListing> {
        let start = match path {
            Some(p) if !p.is_empty() => p.to_owned(),
            _ => home_dir()?,
        };
        self.workspace.list_dirs(&start).await
    }
}
