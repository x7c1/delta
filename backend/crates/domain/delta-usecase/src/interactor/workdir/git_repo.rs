use crate::error::{Error, Result};
use crate::interactor::InteractorCore;
use crate::ports::{
    GitRepoInfo, GitWorktree, RemoteBranches, SessionStore, TmuxDriver, Transcript, Workspace,
};

impl<T, X, S, W, G> InteractorCore<T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Detect whether `path` is inside a git repository, and (if so) the
    /// repository's default branch.
    ///
    /// Backs `GET /api/workdir/git`: the picker calls it for a candidate
    /// directory to decide whether a worktree can be offered. Lightweight — no
    /// fetch. When `path` is not a git repository, `repo_root` is `None` and so
    /// is `default_branch`; otherwise `default_branch` is the repository's
    /// default branch short name when `origin/HEAD` is set.
    pub async fn git_repo_info(&self, path: &str) -> Result<GitRepoInfo> {
        match self.git_worktree.repo_root(path).await? {
            Some(repo_root) => {
                let default_branch = self.git_worktree.default_branch(&repo_root).await?;
                Ok(GitRepoInfo {
                    repo_root: Some(repo_root),
                    default_branch,
                })
            }
            None => Ok(GitRepoInfo {
                repo_root: None,
                default_branch: None,
            }),
        }
    }

    /// Fetch the remote and list the remote branches of the repository
    /// containing `path`, with its default branch highlighted.
    ///
    /// Backs `GET /api/workdir/git/branches`: the branch picker calls it to
    /// offer a remote branch to base a worktree on. Resolves the repository root
    /// first; a `path` that is not inside a git repository is a clean
    /// [`Error::WorktreeNotAGitRepo`] (`400`), distinguishing "not a repo" from a
    /// real git failure. On success it runs a fetch (so the list is current).
    pub async fn git_remote_branches(&self, path: &str) -> Result<RemoteBranches> {
        let repo_root = self
            .git_worktree
            .repo_root(path)
            .await?
            .ok_or_else(|| Error::WorktreeNotAGitRepo(path.to_owned()))?;
        self.git_worktree.fetch_remote_branches(&repo_root).await
    }
}
