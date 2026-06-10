use crate::error::Result;
use crate::ports::{DirListing, SessionStore, TmuxDriver, Transcript, Workspace};
use crate::Interactor;

use super::home_dir;

impl<T, X, S, W> Interactor<T, X, S, W>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
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
