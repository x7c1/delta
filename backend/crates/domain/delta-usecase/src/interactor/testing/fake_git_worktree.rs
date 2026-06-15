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
    /// The default branch `default_branch`/`fetch_remote_branches` report.
    pub(crate) default_branch: Mutex<Option<String>>,
    /// The remote branches `fetch_remote_branches` reports.
    pub(crate) remote_branches: Mutex<Vec<String>>,
    /// When set, `create_worktree` fails instead of recording the call,
    /// simulating a `git worktree add` failure.
    pub(crate) fail_create: bool,
    /// The `create_worktree` calls made, in order.
    pub(crate) created: Mutex<Vec<CreatedWorktree>>,
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

    async fn default_branch(&self, _repo_root: &str) -> Result<Option<String>> {
        Ok(self.default_branch.lock().unwrap().clone())
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

    async fn ensure_dir_trusted(&self, dir: &str) -> Result<()> {
        self.trusted.lock().unwrap().push(dir.to_owned());
        Ok(())
    }
}
