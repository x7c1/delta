//! Rebuilding a closed session's vanished worktree before it is resumed.
//!
//! A session launched in a git worktree keeps its `cwd`, but the directory can
//! be removed while the session is closed — by Delta's own storage cleanup, by
//! another tool, or by hand — while the branch it was on lives on. Resuming
//! there would have tmux start the pane in the home directory, so the resume
//! checks the branch out again **at the same path** first: `claude --resume`
//! finds the transcript and the folder trust by the absolute launch directory,
//! so any other path would lose both.

use delta_model::Session;

use crate::error::{Error, Result};
use crate::interactor::InteractorCore;
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};

use super::worktree_launch_dir::check_ref_name;

/// The repository and branch a vanished worktree is rebuilt from.
struct RebuildSource {
    repo_root: String,
    branch: String,
}

impl<T, X, S, W, G> InteractorCore<T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// When `session`'s `cwd` no longer exists, check the branch it was on out
    /// in a new worktree at that same path; when it exists, do nothing (and
    /// touch no git).
    ///
    /// The session must identify the branch through its spawn-time snapshot:
    /// `repo_root` and `branch_at_launch` both recorded, the repository still a
    /// git repository, and the branch present locally or on `origin`. Whether
    /// Delta created the `cwd` does not matter. Otherwise the resume is refused
    /// with [`Error::WorktreeUnrecoverable`] before anything is created.
    ///
    /// The checkout is [`GitWorktree::restore_worktree_checkout`]. When git
    /// refuses it (the branch is checked out in another worktree that still
    /// exists, say) git's error is returned as it is: the `cwd` stays absent
    /// and the caller creates no pane.
    pub(in crate::interactor) async fn rebuild_vanished_worktree(
        &self,
        session: &Session,
    ) -> Result<()> {
        let cwd = session.cwd.as_str();
        if self.workspace.is_existing_dir(cwd).await? {
            return Ok(());
        }
        let source = self.rebuild_source(session).await?;
        match self
            .git_worktree
            .restore_worktree_checkout(&source.repo_root, cwd, &source.branch)
            .await
        {
            Ok(()) => {
                tracing::info!(
                    session_id = %session.id,
                    path = cwd,
                    branch = %source.branch,
                    "rebuilt the vanished worktree of a closed session before resuming it"
                );
                Ok(())
            }
            Err(err) => {
                tracing::warn!(
                    session_id = %session.id,
                    path = cwd,
                    branch = %source.branch,
                    error = %err,
                    "could not rebuild the vanished worktree of a closed session; \
                     the resume fails"
                );
                Err(err)
            }
        }
    }

    /// The repository and branch `session`'s snapshot identifies, or
    /// [`Error::WorktreeUnrecoverable`] naming why there is none. Runs only
    /// read-only git queries, so a refusal leaves nothing behind.
    async fn rebuild_source(&self, session: &Session) -> Result<RebuildSource> {
        let unrecoverable = |reason: String| Error::WorktreeUnrecoverable {
            dir: session.cwd.clone(),
            reason,
        };
        let Some(repo_root) = session.repo_root.clone() else {
            return Err(unrecoverable(
                "the session recorded no repository at launch".to_owned(),
            ));
        };
        let Some(branch) = session.branch_at_launch.clone() else {
            return Err(unrecoverable(
                "the session recorded no branch at launch".to_owned(),
            ));
        };
        // The name reaches `git` as a positional argument; refuse one git
        // could read as an option, as every other checkout does.
        if check_ref_name(&branch).is_err() {
            return Err(unrecoverable(format!(
                "its branch {branch:?} is not a valid branch name"
            )));
        }
        if self.git_worktree.repo_root(&repo_root).await?.is_none() {
            return Err(unrecoverable(format!(
                "its repository {repo_root} no longer exists or is not a git repository"
            )));
        }
        if !self.git_worktree.has_branch(&repo_root, &branch).await? {
            return Err(unrecoverable(format!(
                "its branch {branch} exists neither locally nor on origin in {repo_root}"
            )));
        }
        Ok(RebuildSource { repo_root, branch })
    }
}
