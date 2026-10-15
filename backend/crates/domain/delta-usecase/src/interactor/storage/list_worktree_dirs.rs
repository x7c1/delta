//! Listing the directories under the worktree base for Settings → Storage.

use std::time::UNIX_EPOCH;

use delta_model::SessionId;

use crate::error::{Error, Result};
use crate::interactor::InteractorCore;
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};
use crate::worktree_branch::WorktreeBranch;
use crate::worktree_dir::WorktreeDir;
use crate::WorktreeInspection;

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
    /// with whether a listed session still works in it, when it was last
    /// modified ([`Self::modified_at`]), and what git says about it: its
    /// repository, whether it holds uncommitted work, the branch checked out
    /// in it ([`Self::worktree_branch`]), and the branch Delta cut for its
    /// session when that is no longer the checked-out one
    /// ([`Self::launch_branch`]).
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
            let (branch, launch_branch) = match &inspection {
                Some(found) => {
                    let branch = self.worktree_branch(&path, found).await?;
                    let checked_out = branch.as_ref().map(|branch| branch.name.as_str());
                    let launch_branch = self
                        .launch_branch(&entry.name, &path, found, checked_out)
                        .await?;
                    (branch, launch_branch)
                }
                None => (None, None),
            };
            let modified_at = self.modified_at(&path).await;
            dirs.push(WorktreeDir {
                in_use,
                modified_at,
                branch,
                launch_branch,
                repo_root: inspection.as_ref().map(|found| found.repo_root.clone()),
                dirty: inspection.map(|found| found.dirty),
                path,
            });
        }
        Ok(dirs)
    }

    /// When the directory at `path` was last modified, as an ISO-8601 UTC
    /// timestamp, or `None` — with a warning — when its metadata cannot be
    /// read: the age only helps the user decide, so a directory without one
    /// is still listed.
    async fn modified_at(&self, path: &str) -> Option<String> {
        let modified = match self.workspace.modified_at(path).await {
            Ok(modified) => modified,
            Err(err) => {
                tracing::warn!(path, error = %err, "could not read a worktree directory's modification time");
                return None;
            }
        };
        match modified.duration_since(UNIX_EPOCH) {
            Ok(since_epoch) => Some(delta_model::iso8601_utc(since_epoch.as_secs())),
            Err(_) => {
                tracing::warn!(
                    path,
                    "a worktree directory's modification time is before the Unix epoch"
                );
                None
            }
        }
    }

    /// The branch checked out in the worktree at `path`, which git `found`,
    /// with whether removing its session would delete it: whether it is the
    /// `delta-<id>` branch of a listed session working in `path`
    /// ([`Self::launched_on_its_branch`]), and then, when the worktree is
    /// clean, whether it is merged.
    async fn worktree_branch(
        &self,
        path: &str,
        found: &WorktreeInspection,
    ) -> Result<Option<WorktreeBranch>> {
        let Some(name) = self.git_worktree.current_branch(path).await? else {
            return Ok(None);
        };
        let created_for_session = match name.strip_prefix("delta-") {
            Some(id) => self.launched_on_its_branch(id, path).await?,
            None => false,
        };
        let merged = if created_for_session && !found.dirty {
            self.git_worktree
                .is_branch_merged(&found.repo_root, &name)
                .await?
        } else {
            None
        };
        Ok(Some(WorktreeBranch {
            name,
            created_for_session,
            merged,
        }))
    }

    /// The `delta-<id>` branch Delta cut for the session that created the
    /// worktree at `path` (directory `dir_name`), when another branch, or a
    /// detached `HEAD`, is `checked_out` there now and it still exists: the
    /// session switched branches, and removing it settles this branch too.
    ///
    /// Delta cuts `delta-<id>` only at `<worktree base>/<slug>-<id>`, so the
    /// session is found by trying each suffix of `dir_name` after a `-` as an
    /// id ([`Self::launched_on_its_branch`]).
    async fn launch_branch(
        &self,
        dir_name: &str,
        path: &str,
        found: &WorktreeInspection,
        checked_out: Option<&str>,
    ) -> Result<Option<WorktreeBranch>> {
        for (dash, _) in dir_name.match_indices('-') {
            let id = &dir_name[dash + 1..];
            if id.is_empty() {
                continue;
            }
            let name = format!("delta-{id}");
            if checked_out == Some(name.as_str()) {
                return Ok(None);
            }
            if !self.launched_on_its_branch(id, path).await? {
                continue;
            }
            // Asked even in a dirty worktree, where the answer decides
            // nothing, to leave out a branch the user already deleted.
            let Some(merged) = self
                .git_worktree
                .is_branch_merged(&found.repo_root, &name)
                .await?
            else {
                return Ok(None);
            };
            return Ok(Some(WorktreeBranch {
                name,
                created_for_session: true,
                merged: (!found.dirty).then_some(merged),
            }));
        }
        Ok(None)
    }

    /// Whether session `id` is listed, works in `path` and was launched on
    /// its own `delta-<id>` branch — the test removing it applies to its row
    /// before deleting that branch.
    async fn launched_on_its_branch(&self, id: &str, path: &str) -> Result<bool> {
        let branch = format!("delta-{id}");
        Ok(self
            .store
            .session(&SessionId::from(id))
            .await?
            .is_some_and(|session| {
                session.cwd == path && session.branch_at_launch.as_deref() == Some(&branch)
            }))
    }
}
