//! In-memory [`GitWorktree`] fake recording the calls the interactor makes and
//! modelling a small set of "git repositories" for the worktree spawn path.

use std::sync::Mutex;

use async_trait::async_trait;

use crate::error::Result;
use crate::ports::{GitWorktree, RemoteBranches, WorktreeStartPoint};

/// A single recorded `create_worktree` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CreatedWorktree {
    pub(crate) repo_root: String,
    pub(crate) worktree_path: String,
    pub(crate) branch: String,
    pub(crate) start_point: WorktreeStartPoint,
}

/// A single recorded `add_worktree_checkout` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CheckedOutWorktree {
    pub(crate) repo_root: String,
    pub(crate) worktree_path: String,
    pub(crate) branch: String,
}

/// Models git detection and worktree creation for the use-case tests.
///
/// `repo_roots` maps a (resolved) directory to the repository root
/// `repo_root` should return for it; a directory not in the map is "not a git
/// repo" (`Ok(None)`). `create_worktree` records its arguments unless
/// `fail_create` is set, in which case it fails like a real `git worktree add`
/// error.
#[derive(Default)]
pub(crate) struct FakeGitWorktree {
    /// Directories that "are" git repositories, mapping each to the root
    /// `repo_root` returns. Anything absent resolves to `None` (not a git repo).
    pub(crate) repo_roots: Mutex<Vec<(String, String)>>,
    /// Branch names `current_branch` should return for a given directory. A
    /// directory absent from the map resolves to `None` (no branch at launch:
    /// not a git repo, or detached HEAD). A present entry mirrors the real
    /// gateway's branch-name short form.
    pub(crate) current_branches: Mutex<Vec<(String, String)>>,
    /// The remote `origin` URL `origin_url` should return for a given
    /// directory. A directory absent from the map resolves to `None`
    /// ("origin unset" or "not a git repo"). Mirrors the real gateway's
    /// behavior, which reads `remote.origin.url` from the shared `.git/config`.
    pub(crate) origin_urls: Mutex<Vec<(String, String)>>,
    /// The default branch `default_branch`/`fetch_remote_branches` report.
    pub(crate) default_branch: Mutex<Option<String>>,
    /// The remote branches `fetch_remote_branches` reports.
    pub(crate) remote_branches: Mutex<Vec<String>>,
    /// When set, `create_worktree` fails instead of recording the call,
    /// simulating a `git worktree add` failure.
    pub(crate) fail_create: bool,
    /// The `create_worktree` calls made, in order.
    pub(crate) created: Mutex<Vec<CreatedWorktree>>,
    /// Scripted results for `worktree_path_for_branch`, keyed by branch name.
    /// A branch absent from the map resolves to `None` ("not checked out
    /// anywhere"); a present `(branch, path)` makes the fake report `path` as
    /// the worktree already holding that branch (driving the reuse path).
    pub(crate) checked_out_branches: Mutex<Vec<(String, String)>>,
    /// The `add_worktree_checkout` calls made, in order.
    pub(crate) checked_out: Mutex<Vec<CheckedOutWorktree>>,
    /// The dirs passed to `ensure_dir_trusted`, in order, so tests can assert
    /// whether (and with what path) trust-seeding was invoked.
    pub(crate) trusted: Mutex<Vec<String>>,
}

impl FakeGitWorktree {
    /// Register `dir` as a git repository rooted at `root`.
    pub(crate) fn with_repo(self, dir: &str, root: &str) -> Self {
        self.repo_roots
            .lock()
            .unwrap()
            .push((dir.to_owned(), root.to_owned()));
        self
    }

    /// Script `current_branch(dir)` to report `branch`.
    pub(crate) fn with_current_branch(self, dir: &str, branch: &str) -> Self {
        self.current_branches
            .lock()
            .unwrap()
            .push((dir.to_owned(), branch.to_owned()));
        self
    }

    /// Script `origin_url(dir)` to report `url`.
    pub(crate) fn with_origin_url(self, dir: &str, url: &str) -> Self {
        self.origin_urls
            .lock()
            .unwrap()
            .push((dir.to_owned(), url.to_owned()));
        self
    }

    /// Script `worktree_path_for_branch(_, branch)` to report `path` — i.e.
    /// `branch` is already checked out in the worktree at `path` (driving the
    /// `UseRemoteBranch` reuse path).
    pub(crate) fn with_branch_checked_out(self, branch: &str, path: &str) -> Self {
        self.checked_out_branches
            .lock()
            .unwrap()
            .push((branch.to_owned(), path.to_owned()));
        self
    }
}

#[async_trait]
impl GitWorktree for FakeGitWorktree {
    async fn repo_root(&self, path: &str) -> Result<Option<String>> {
        Ok(self
            .repo_roots
            .lock()
            .unwrap()
            .iter()
            .find(|(dir, _)| dir == path)
            .map(|(_, root)| root.clone()))
    }

    async fn current_branch(&self, path: &str) -> Result<Option<String>> {
        Ok(self
            .current_branches
            .lock()
            .unwrap()
            .iter()
            .find(|(dir, _)| dir == path)
            .map(|(_, branch)| branch.clone()))
    }

    async fn default_branch(&self, _repo_root: &str) -> Result<Option<String>> {
        Ok(self.default_branch.lock().unwrap().clone())
    }

    async fn origin_url(&self, path: &str) -> Result<Option<String>> {
        Ok(self
            .origin_urls
            .lock()
            .unwrap()
            .iter()
            .find(|(dir, _)| dir == path)
            .map(|(_, url)| url.clone()))
    }

    async fn fetch_remote_branches(&self, _repo_root: &str) -> Result<RemoteBranches> {
        Ok(RemoteBranches {
            default_branch: self.default_branch.lock().unwrap().clone(),
            branches: self.remote_branches.lock().unwrap().clone(),
        })
    }

    async fn create_worktree(
        &self,
        repo_root: &str,
        worktree_path: &str,
        branch: &str,
        start_point: WorktreeStartPoint,
    ) -> Result<()> {
        if self.fail_create {
            return Err(crate::error::Error::Git("worktree add failed".into()));
        }
        self.created.lock().unwrap().push(CreatedWorktree {
            repo_root: repo_root.to_owned(),
            worktree_path: worktree_path.to_owned(),
            branch: branch.to_owned(),
            start_point,
        });
        Ok(())
    }

    async fn worktree_path_for_branch(
        &self,
        _repo_root: &str,
        branch: &str,
    ) -> Result<Option<String>> {
        Ok(self
            .checked_out_branches
            .lock()
            .unwrap()
            .iter()
            .find(|(name, _)| name == branch)
            .map(|(_, path)| path.clone()))
    }

    async fn add_worktree_checkout(
        &self,
        repo_root: &str,
        worktree_path: &str,
        branch: &str,
    ) -> Result<()> {
        if self.fail_create {
            return Err(crate::error::Error::Git("worktree add failed".into()));
        }
        self.checked_out.lock().unwrap().push(CheckedOutWorktree {
            repo_root: repo_root.to_owned(),
            worktree_path: worktree_path.to_owned(),
            branch: branch.to_owned(),
        });
        Ok(())
    }

    async fn ensure_dir_trusted(&self, dir: &str) -> Result<()> {
        self.trusted.lock().unwrap().push(dir.to_owned());
        Ok(())
    }
}
