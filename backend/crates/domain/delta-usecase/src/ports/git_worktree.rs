//! Detecting git repositories and creating per-session git worktrees.

use async_trait::async_trait;

use crate::error::Result;

/// Where a new worktree's branch should start from.
///
/// A fresh session opts into a worktree by naming a start point: branch off the
/// current checkout (`Head`), or off a named remote branch (`RemoteBranch`,
/// which is fetched first so the worktree always starts from the latest remote
/// tip).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorktreeStartPoint {
    /// Branch off the repository's current `HEAD`. No network access.
    Head,
    /// Branch off `origin/<name>`, fetched first so it reflects the latest
    /// remote tip. The carried name is the remote branch's short name (no
    /// `origin/` prefix), e.g. `main`.
    RemoteBranch(String),
}

/// Git facts about a candidate working directory, for the detection endpoint.
///
/// `repo_root` is the repository root containing the queried path (`None` when
/// it is not inside a git repository), and `default_branch` is that
/// repository's default branch short name when known. Computed without any
/// network access (no fetch).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitRepoInfo {
    /// The repository root containing the queried path, or `None` when the path
    /// is not inside a git repository.
    pub repo_root: Option<String>,
    /// The repository's default branch short name (e.g. `main`), or `None` when
    /// it is unset or the path is not a git repository.
    pub default_branch: Option<String>,
}

/// The remote branches of a repository, with its default branch highlighted.
///
/// Returned by [`GitWorktree::fetch_remote_branches`] to back the branch picker:
/// `branches` are the remote branch short names (no `origin/` prefix), and
/// `default_branch` is the repository's default branch short name when it can be
/// determined (`None` when `origin/HEAD` is unset).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteBranches {
    /// The repository's default branch short name (e.g. `main`), or `None` when
    /// `origin/HEAD` is unset.
    pub default_branch: Option<String>,
    /// The remote branch short names (no `origin/` prefix), excluding the
    /// `origin/HEAD` symref entry.
    pub branches: Vec<String>,
}

/// Detects git repositories and creates per-session git worktrees.
///
/// All `git` subprocess interaction is isolated behind this port: the domain
/// asks for repository facts (is this a git repo, what is its default branch,
/// what remote branches exist) and for a worktree to be created, and the
/// gateway shells out to `git`. Every method takes the repository path
/// explicitly (the gateway always invokes `git -C <repo>`), so the port is
/// stateless and cwd-independent — mirroring the [`TmuxDriver`] port.
///
/// [`TmuxDriver`]: crate::ports::TmuxDriver
#[async_trait]
pub trait GitWorktree: Send + Sync {
    /// The repository root containing `path`, or `None` when `path` is not
    /// inside a git repository.
    ///
    /// Runs `git -C <path> rev-parse --show-toplevel`: a non-zero exit (not a
    /// git repo) is the `None` signal, not an error to propagate. The returned
    /// root has any trailing newline trimmed. Lightweight: no fetch.
    async fn repo_root(&self, path: &str) -> Result<Option<String>>;

    /// The repository's default branch short name (e.g. `main`), or `None` when
    /// it is unset.
    ///
    /// Runs `git -C <repo_root> symbolic-ref --short refs/remotes/origin/HEAD`
    /// (e.g. `origin/main`) and strips the `origin/` prefix. A non-zero exit
    /// (`origin/HEAD` unset) is the `None` signal. Best-effort: no fetch.
    async fn default_branch(&self, repo_root: &str) -> Result<Option<String>>;

    /// Fetch the remote and list its branches, recomputing the default branch.
    ///
    /// Runs `git -C <repo_root> fetch --prune` first (the "always latest" path),
    /// then lists remote branches via `for-each-ref` over `refs/remotes/origin`
    /// (excluding the `origin/HEAD` symref) with the `origin/` prefix stripped,
    /// and recomputes the default branch.
    async fn fetch_remote_branches(&self, repo_root: &str) -> Result<RemoteBranches>;

    /// Create a new worktree at `worktree_path` on a new branch `branch`,
    /// starting from `start_point`.
    ///
    /// For [`WorktreeStartPoint::Head`]:
    /// `git -C <repo_root> worktree add -b <branch> <worktree_path> HEAD`.
    /// For [`WorktreeStartPoint::RemoteBranch`]: first
    /// `git -C <repo_root> fetch origin <name>`, then
    /// `git -C <repo_root> worktree add -b <branch> <worktree_path> origin/<name>`.
    /// A `git` failure surfaces its stderr in the error.
    async fn create_worktree(
        &self,
        repo_root: &str,
        worktree_path: &str,
        branch: &str,
        start_point: WorktreeStartPoint,
    ) -> Result<()>;

    /// Ensure `dir` is marked trusted in Claude Code's user config so the
    /// interactive workspace-trust dialog does not block a programmatic launch in
    /// a fresh directory. Writes `projects.<dir>.hasTrustDialogAccepted = true` to
    /// the user's `~/.claude.json`. Idempotent: a no-op if already trusted.
    ///
    /// This trust-seeding concern is folded into the `GitWorktree` port (rather
    /// than its own port) deliberately: a separate port would add another generic
    /// type parameter to `Interactor`/`SessionContext`, rippling across many
    /// files. Both this and the git facts above are "what the gateway knows about
    /// git working directories", so they share one port.
    async fn ensure_dir_trusted(&self, dir: &str) -> Result<()>;
}

#[async_trait]
impl GitWorktree for Box<dyn GitWorktree> {
    async fn repo_root(&self, path: &str) -> Result<Option<String>> {
        (**self).repo_root(path).await
    }

    async fn default_branch(&self, repo_root: &str) -> Result<Option<String>> {
        (**self).default_branch(repo_root).await
    }

    async fn fetch_remote_branches(&self, repo_root: &str) -> Result<RemoteBranches> {
        (**self).fetch_remote_branches(repo_root).await
    }

    async fn create_worktree(
        &self,
        repo_root: &str,
        worktree_path: &str,
        branch: &str,
        start_point: WorktreeStartPoint,
    ) -> Result<()> {
        (**self)
            .create_worktree(repo_root, worktree_path, branch, start_point)
            .await
    }

    async fn ensure_dir_trusted(&self, dir: &str) -> Result<()> {
        (**self).ensure_dir_trusted(dir).await
    }
}
