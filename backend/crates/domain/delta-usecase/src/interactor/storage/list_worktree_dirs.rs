//! Listing the directories under the worktree base for Settings → Storage.

use crate::error::{Error, Result};
use crate::interactor::InteractorCore;
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};
use crate::worktree_dir::WorktreeDir;

use super::child_path;

impl<T, X, S, W, G> InteractorCore<T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Every directory directly under the worktree base, sorted by name, each
    /// with whether a listed session still works in it and what git says
    /// about it.
    ///
    /// "Works in it" is the check removing a session uses to keep a worktree
    /// another session works in: the path is some session's `cwd` or
    /// requested directory, or some message's `cwd`. The directories nothing
    /// works in are the leftovers — worktrees kept because they held work
    /// when their session was removed, or left by versions that did not clean
    /// up.
    ///
    /// A worktree base that does not exist yet (no worktree session has run)
    /// lists nothing. Any other failure — the base cannot be read, the store
    /// or `git` fails — fails the listing rather than describing a directory
    /// wrongly.
    pub async fn list_worktree_dirs(&self) -> Result<Vec<WorktreeDir>> {
        let base = self.worktree_base.as_str();
        let listing = match self.workspace.list_dirs(base, true).await {
            Ok(listing) => listing,
            Err(Error::InvalidWorkdir(reason)) => {
                tracing::debug!(base, reason, "no worktree base to list");
                return Ok(Vec::new());
            }
            Err(err) => return Err(err),
        };
        let mut dirs = Vec::with_capacity(listing.entries.len());
        for entry in listing.entries {
            let path = child_path(base, &entry.name);
            let in_use = self.store.cwd_exists(&path).await?;
            let inspection = self.git_worktree.inspect_worktree(&path).await?;
            dirs.push(WorktreeDir {
                in_use,
                repo_root: inspection.as_ref().map(|found| found.repo_root.clone()),
                dirty: inspection.map(|found| found.dirty),
                path,
            });
        }
        Ok(dirs)
    }
}
