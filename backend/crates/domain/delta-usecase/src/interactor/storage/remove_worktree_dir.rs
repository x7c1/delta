//! Removing a leftover worktree from Settings → Storage.

use crate::error::{Error, Result};
use crate::interactor::InteractorCore;
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace, WorktreeRemoval};

use super::is_directly_under;

impl<T, X, S, W, G> InteractorCore<T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Remove the directory `path` under the worktree base, which no listed
    /// session works in — destroying its changes only when `force` says the
    /// user confirmed that.
    ///
    /// Refused, with nothing touched:
    ///
    /// - [`Error::WorktreeOutsideBase`] — `path` is not a directory directly
    ///   under the worktree base (as [`Self::list_worktree_dirs`] spells it).
    /// - [`Error::WorktreeInUse`] — a listed session still works in it.
    /// - [`Error::WorktreeDirty`] — without `force`, it has uncommitted or
    ///   untracked changes.
    /// - [`Error::WorktreeNotRegistered`] — without `force`, git no longer
    ///   knows it as a worktree, so nothing can say it holds no work.
    ///
    /// A clean worktree is removed the way removing a session removes one
    /// (`git worktree remove`, no `--force`); with `force` a dirty one goes
    /// through `git worktree remove --force`, and a directory git no longer
    /// knows is deleted as a plain directory tree. The worktree's branch is
    /// never touched. After the removal `git worktree prune` runs in the
    /// repository, when git named one, and the `~/.claude.json` trust entry
    /// Delta seeded for the path is forgotten; a failure there is logged and
    /// does not fail the removal, which has already happened.
    pub async fn remove_worktree_dir(&self, path: &str, force: bool) -> Result<()> {
        if !is_directly_under(&self.worktree_base, path) {
            return Err(Error::WorktreeOutsideBase(path.to_owned()));
        }
        if self.store.cwd_exists(path).await? {
            return Err(Error::WorktreeInUse(path.to_owned()));
        }
        let repo_root = match self.git_worktree.inspect_worktree(path).await? {
            Some(inspection) => {
                if inspection.dirty && !force {
                    return Err(Error::WorktreeDirty(path.to_owned()));
                }
                self.remove_registered_worktree(
                    &inspection.repo_root,
                    path,
                    inspection.dirty,
                    force,
                )
                .await?;
                Some(inspection.repo_root)
            }
            None => {
                if !force {
                    return Err(Error::WorktreeNotRegistered(path.to_owned()));
                }
                self.workspace.remove_dir_tree(path).await?;
                None
            }
        };
        tracing::info!(path, repo_root, force, "removed a leftover worktree");

        if let Some(repo_root) = repo_root.as_deref() {
            if let Err(err) = self.git_worktree.prune_worktrees(repo_root).await {
                tracing::warn!(
                    repo_root,
                    error = %err,
                    "git worktree prune failed after removing a leftover worktree"
                );
            }
        }
        if let Err(err) = self.git_worktree.forget_dir_trusted(path).await {
            tracing::warn!(
                path,
                error = %err,
                "forgetting the removed worktree's trust entry failed"
            );
        }
        Ok(())
    }

    /// Remove a worktree git knows: without `--force` when it looked clean,
    /// so git's own check still stands between Delta and any work that
    /// appeared since; with it only when `force` was given. A clean-looking
    /// worktree that git refuses has gained work in the meantime, and is
    /// refused like a dirty one unless `force` was given.
    async fn remove_registered_worktree(
        &self,
        repo_root: &str,
        path: &str,
        dirty: bool,
        force: bool,
    ) -> Result<()> {
        if !dirty {
            match self.git_worktree.remove_worktree(repo_root, path).await? {
                WorktreeRemoval::Removed => return Ok(()),
                WorktreeRemoval::KeptDirty if !force => {
                    return Err(Error::WorktreeDirty(path.to_owned()));
                }
                WorktreeRemoval::KeptDirty => {}
            }
        }
        self.git_worktree
            .force_remove_worktree(repo_root, path)
            .await
    }
}
