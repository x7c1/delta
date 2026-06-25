//! Detecting git repositories and creating per-session git worktrees.

use async_trait::async_trait;

use crate::error::Result;

/// Where a new session's worktree should start from, and whether it gets its
/// own `delta-<id>` branch or works on an existing branch directly.
///
/// A fresh session opts into a worktree by naming a start point:
/// - `Head` / `RemoteBranch` cut a *new* `delta-<id>` branch — from the current
///   checkout, or from a named remote branch (fetched first so the worktree
///   always starts from the latest remote tip).
/// - `UseRemoteBranch` instead works on the named branch *itself* in the
///   worktree (no `delta-<id>` branch). Because git forbids checking one branch
///   out in two worktrees, the orchestration reuses the worktree that already
///   has that branch checked out (including the main working tree) when one
///   exists, and otherwise creates a worktree that checks the branch out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorktreeStartPoint {
    /// Cut a new `delta-<id>` branch off the repository's current `HEAD`. No
    /// network access.
    Head,
    /// Cut a new `delta-<id>` branch off `origin/<name>`, fetched first so it
    /// reflects the latest remote tip. The carried name is the remote branch's
    /// short name (no `origin/` prefix), e.g. `main`.
    RemoteBranch(String),
    /// Work on the branch `<name>` itself in the worktree (no `delta-<id>`
    /// branch). The carried name is the branch's short name (no `origin/`
    /// prefix). The orchestration reuses an existing worktree that already has
    /// `<name>` checked out, or creates one that checks it out (creating a
    /// local tracking branch off `origin/<name>` first when no local branch
    /// exists yet).
    UseRemoteBranch(String),
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
    /// The working-tree root containing `path`, or `None` when `path` is not
    /// inside a git repository.
    ///
    /// Runs `git -C <path> rev-parse --show-toplevel`: a non-zero exit (not a
    /// git repo) is the `None` signal, not an error to propagate. The returned
    /// root has any trailing newline trimmed. Lightweight: no fetch.
    ///
    /// Note: this returns the **working tree's** top, which is the linked
    /// worktree path itself when `path` lives inside a linked git worktree —
    /// not the original clone. For a cross-worktree repository identity,
    /// pair this with [`Self::origin_url`] and the
    /// [`crate::identity_key`] / [`crate::display_name`] helpers.
    async fn repo_root(&self, path: &str) -> Result<Option<String>>;

    /// The local branch name currently checked out under `path`, or `None`
    /// when `path` is not inside a git repository OR when HEAD is detached.
    ///
    /// Runs `git -C <path> rev-parse --abbrev-ref HEAD`: a non-zero exit (not a
    /// git repo) and an output of the literal `HEAD` (detached) are both the
    /// `None` signal, not errors to propagate. The returned name has any
    /// trailing newline trimmed. Lightweight: no fetch. Mirrors the
    /// `Option`-returning shape of [`Self::repo_root`].
    async fn current_branch(&self, path: &str) -> Result<Option<String>>;

    /// The repository's default branch short name (e.g. `main`), or `None` when
    /// it is unset.
    ///
    /// Runs `git -C <repo_root> symbolic-ref --short refs/remotes/origin/HEAD`
    /// (e.g. `origin/main`) and strips the `origin/` prefix. A non-zero exit
    /// (`origin/HEAD` unset) is the `None` signal. Best-effort: no fetch.
    async fn default_branch(&self, repo_root: &str) -> Result<Option<String>>;

    /// The repository's `origin` remote URL (e.g. `git@github.com:x7c1/delta`
    /// or `https://github.com/x7c1/delta.git`), or `None` when `remote.origin.url`
    /// is unset (or the path is not inside a git repository).
    ///
    /// Runs `git -C <path> config --get remote.origin.url`: a non-zero exit
    /// (no remote, or not a git repo) is the `None` signal, not an error to
    /// propagate. The returned URL has any trailing newline trimmed.
    /// Lightweight: no fetch.
    ///
    /// Because `remote.origin.url` lives in the shared `.git/config`, calling
    /// this from a linked worktree returns the same URL as the main working
    /// tree — repository identity is stable across worktrees. Feeds the
    /// repository-identity helpers ([`crate::identity_key`] /
    /// [`crate::display_name`]) used by the Repository tab to bundle multiple
    /// local clones of the same upstream under one identity, and by the
    /// navigator to render a short `org/repo` label on each session card.
    async fn origin_url(&self, path: &str) -> Result<Option<String>>;

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

    /// The absolute path of the worktree that currently has local branch
    /// `branch` checked out, or `None` when no worktree has it checked out.
    ///
    /// Parses `git -C <repo_root> worktree list --porcelain`, matching the
    /// `branch refs/heads/<branch>` entry against the *local* branch short
    /// name. This includes the main working tree, so a branch checked out in
    /// the main tree is reported (and can be reused) just like one in a
    /// secondary worktree. Backs the `UseRemoteBranch` reuse path.
    async fn worktree_path_for_branch(
        &self,
        repo_root: &str,
        branch: &str,
    ) -> Result<Option<String>>;

    /// Create a new worktree at `worktree_path` that checks out the existing
    /// local branch `branch` itself (NOT a fresh `delta-<id>` branch).
    ///
    /// First `git -C <repo_root> fetch origin <branch>` so the branch reflects
    /// the latest remote tip. Then, if local branch `branch` already exists,
    /// `git -C <repo_root> worktree add <worktree_path> <branch>`; otherwise
    /// `git -C <repo_root> worktree add --track -b <branch> <worktree_path>
    /// origin/<branch>`, creating a local branch tracking the remote one and
    /// checking it out. The parent of `worktree_path` is created first (the
    /// worktree base may not exist yet), mirroring [`Self::create_worktree`].
    /// A `git` failure surfaces its stderr in the error.
    async fn add_worktree_checkout(
        &self,
        repo_root: &str,
        worktree_path: &str,
        branch: &str,
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

    async fn current_branch(&self, path: &str) -> Result<Option<String>> {
        (**self).current_branch(path).await
    }

    async fn default_branch(&self, repo_root: &str) -> Result<Option<String>> {
        (**self).default_branch(repo_root).await
    }

    async fn origin_url(&self, path: &str) -> Result<Option<String>> {
        (**self).origin_url(path).await
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

    async fn worktree_path_for_branch(
        &self,
        repo_root: &str,
        branch: &str,
    ) -> Result<Option<String>> {
        (**self).worktree_path_for_branch(repo_root, branch).await
    }

    async fn add_worktree_checkout(
        &self,
        repo_root: &str,
        worktree_path: &str,
        branch: &str,
    ) -> Result<()> {
        (**self)
            .add_worktree_checkout(repo_root, worktree_path, branch)
            .await
    }

    async fn ensure_dir_trusted(&self, dir: &str) -> Result<()> {
        (**self).ensure_dir_trusted(dir).await
    }
}
