//! [`Git`]: the concrete [`GitWorktree`].

use std::path::PathBuf;

use async_trait::async_trait;
use tokio::process::Command;
use tokio::sync::Mutex;

use delta_usecase::{GitWorktree, RemoteBranches, WorktreeStartPoint};

use crate::error::Error;

/// The `origin/` prefix `git` uses for remote-tracking refs, stripped to return
/// branch short names.
const ORIGIN_PREFIX: &str = "origin/";

/// The remote name Delta operates against. Worktrees branch off `origin/<name>`
/// and the default branch is read from `origin/HEAD`.
const REMOTE: &str = "origin";

/// Detects git repositories and creates per-session git worktrees by shelling
/// out to `git`.
///
/// The git operations are stateless: every git method takes the repository (or
/// candidate) path explicitly and invokes `git -C <path> …`, so the gateway is
/// cwd-independent — it never relies on the process's current directory. This
/// mirrors the tmux driver, the project's other subprocess gateway.
///
/// In addition to git, the gateway owns Claude Code's user-config path
/// (`config_path`) and a [`Mutex`] that serializes Delta's own read-modify-write
/// of that file when seeding workspace trust (see [`GitWorktree::ensure_dir_trusted`]).
#[derive(Debug)]
pub struct Git {
    /// Path to Claude Code's user config (`~/.claude.json` by default). Held as a
    /// field so tests can point it at a temp file instead of the real config.
    config_path: PathBuf,
    /// Serializes Delta's own trust-config writes within this single process.
    trust_lock: Mutex<()>,
}

impl Default for Git {
    fn default() -> Self {
        Self::new()
    }
}

impl Git {
    /// Create a new git worktree gateway, defaulting the trust-config path to
    /// `$HOME/.claude.json` (falling back to `.claude.json` in the current
    /// directory if `HOME` is unset, which only happens in degenerate
    /// environments).
    pub fn new() -> Self {
        let config_path = match std::env::var_os("HOME") {
            Some(home) => PathBuf::from(home).join(".claude.json"),
            None => PathBuf::from(".claude.json"),
        };
        Self::with_config_path(config_path)
    }

    /// Create a gateway with an explicit trust-config path. Used by tests to
    /// target a temp file instead of the real `~/.claude.json`.
    pub fn with_config_path(config_path: PathBuf) -> Self {
        Self {
            config_path,
            trust_lock: Mutex::new(()),
        }
    }

    /// Run `git -C <repo> <args>`, returning the captured output.
    async fn output(
        &self,
        repo: &str,
        args: &[&str],
    ) -> std::result::Result<std::process::Output, Error> {
        Ok(Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .await?)
    }

    /// Run `git -C <repo> <args>`, erroring on a non-zero exit with git's
    /// stderr surfaced. `label` names the command for the error message.
    async fn run(&self, repo: &str, label: &str, args: &[&str]) -> std::result::Result<(), Error> {
        let output = self.output(repo, args).await?;
        if output.status.success() {
            Ok(())
        } else {
            Err(command_error(label, &output))
        }
    }
}

/// Build the non-zero-exit error for a `git` command, carrying its stderr.
fn command_error(label: &str, output: &std::process::Output) -> Error {
    Error::Command {
        command: label.to_owned(),
        status: output.status.to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
    }
}

/// The trimmed stdout of a successful `git` command.
fn trimmed_stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// Strip the `origin/` prefix from a remote-tracking ref short name.
fn strip_origin(name: &str) -> &str {
    name.strip_prefix(ORIGIN_PREFIX).unwrap_or(name)
}

/// Create the parent directory of `worktree_path` if it is missing.
///
/// `git worktree add <path>` requires the *parent* of `<path>` to already exist
/// (it creates the leaf, not the chain above it). Worktrees live under a neutral
/// base outside any repo tree (`DELTA_WORKTREE_BASE`, default
/// `$HOME/.delta/worktrees`), which may not exist on a fresh install, so the
/// parent is created here, at the point the worktree is made.
async fn ensure_worktree_parent_dir(worktree_path: &str) -> std::result::Result<(), Error> {
    if let Some(parent) = std::path::Path::new(worktree_path).parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|source| Error::WorktreeBaseIo {
                path: parent.to_string_lossy().into_owned(),
                source,
            })?;
    }
    Ok(())
}

#[async_trait]
impl GitWorktree for Git {
    async fn repo_root(
        &self,
        path: &str,
    ) -> std::result::Result<Option<String>, delta_usecase::Error> {
        // `rev-parse --show-toplevel` prints the repository root and exits 0
        // inside a repo; outside one it exits non-zero. A non-zero exit is the
        // expected "not a git repo" signal, not an error to propagate.
        let output = self
            .output(path, &["rev-parse", "--show-toplevel"])
            .await
            .map_err(delta_usecase::Error::from)?;
        if output.status.success() {
            Ok(Some(trimmed_stdout(&output)))
        } else {
            Ok(None)
        }
    }

    async fn current_branch(
        &self,
        path: &str,
    ) -> std::result::Result<Option<String>, delta_usecase::Error> {
        // `rev-parse --abbrev-ref HEAD` prints the branch short name when HEAD
        // resolves to a local branch, and the literal `HEAD` when HEAD is
        // detached. A non-zero exit (path is not inside a git repo) is the
        // expected `None` signal, not an error to propagate; the detached-HEAD
        // case is also reported as `None` because there is no branch name to
        // record at launch time.
        let output = self
            .output(path, &["rev-parse", "--abbrev-ref", "HEAD"])
            .await
            .map_err(delta_usecase::Error::from)?;
        if !output.status.success() {
            return Ok(None);
        }
        let name = trimmed_stdout(&output);
        if name == "HEAD" {
            Ok(None)
        } else {
            Ok(Some(name))
        }
    }

    async fn default_branch(
        &self,
        repo_root: &str,
    ) -> std::result::Result<Option<String>, delta_usecase::Error> {
        // `symbolic-ref --short refs/remotes/origin/HEAD` prints `origin/<name>`
        // when the remote's default branch is known, and exits non-zero when
        // `origin/HEAD` is unset. A non-zero exit is the expected "unset" signal.
        let output = self
            .output(
                repo_root,
                &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
            )
            .await
            .map_err(delta_usecase::Error::from)?;
        if output.status.success() {
            Ok(Some(strip_origin(&trimmed_stdout(&output)).to_owned()))
        } else {
            Ok(None)
        }
    }

    async fn fetch_remote_branches(
        &self,
        repo_root: &str,
    ) -> std::result::Result<RemoteBranches, delta_usecase::Error> {
        // Fetch first (with prune so deleted remote branches disappear) so the
        // listing reflects the latest remote tip — the "always latest" path.
        self.run(repo_root, "fetch --prune", &["fetch", "--prune"])
            .await
            .map_err(delta_usecase::Error::from)?;

        // List remote branches by short name. `origin/HEAD` is a symref, not a
        // branch, so exclude it from the list.
        let output = self
            .output(
                repo_root,
                &[
                    "for-each-ref",
                    "--format=%(refname:short)",
                    "refs/remotes/origin",
                ],
            )
            .await
            .map_err(delta_usecase::Error::from)?;
        if !output.status.success() {
            return Err(command_error("for-each-ref refs/remotes/origin", &output).into());
        }
        let branches = trimmed_stdout(&output)
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            // Drop the `origin/HEAD` symref entry; only real branches remain.
            .filter(|line| *line != "origin/HEAD")
            .map(|line| strip_origin(line).to_owned())
            .collect();

        let default_branch = self.default_branch(repo_root).await?;

        Ok(RemoteBranches {
            default_branch,
            branches,
        })
    }

    async fn create_worktree(
        &self,
        repo_root: &str,
        worktree_path: &str,
        branch: &str,
        start_point: WorktreeStartPoint,
    ) -> std::result::Result<(), delta_usecase::Error> {
        // Resolve the worktree's start commit. For a remote branch, fetch it
        // first so the worktree starts from the latest remote tip, then branch
        // off `origin/<name>`. For `Head`, branch off the current checkout.
        let start_ref = match &start_point {
            WorktreeStartPoint::Head => "HEAD".to_owned(),
            WorktreeStartPoint::RemoteBranch(name) => {
                self.run(repo_root, "fetch origin <branch>", &["fetch", REMOTE, name])
                    .await
                    .map_err(delta_usecase::Error::from)?;
                format!("{ORIGIN_PREFIX}{name}")
            }
            // `UseRemoteBranch` works on an existing branch directly and is
            // routed to `add_worktree_checkout` by the orchestration, never to
            // `create_worktree` (which always cuts a fresh `delta-<id>` branch).
            WorktreeStartPoint::UseRemoteBranch(name) => {
                return Err(delta_usecase::Error::Git(format!(
                    "create_worktree cannot cut a new branch for UseRemoteBranch({name}); \
                     this start point must be routed to add_worktree_checkout"
                )));
            }
        };

        ensure_worktree_parent_dir(worktree_path).await?;

        // `worktree add -b <branch> <path> <start_ref>` creates the worktree on
        // a fresh branch rooted at the start ref. git's stderr is surfaced on
        // failure (e.g. the branch already exists, or the path is occupied).
        self.run(
            repo_root,
            "worktree add",
            &["worktree", "add", "-b", branch, worktree_path, &start_ref],
        )
        .await
        .map_err(delta_usecase::Error::from)
    }

    async fn worktree_path_for_branch(
        &self,
        repo_root: &str,
        branch: &str,
    ) -> std::result::Result<Option<String>, delta_usecase::Error> {
        // `worktree list --porcelain` emits a block per worktree: a
        // `worktree <abs-path>` line, then (for a normal checkout) a
        // `branch refs/heads/<name>` line; a detached or bare entry has no
        // branch line. Pair each path with the immediately following branch
        // line and return the path whose local branch matches `<branch>`. The
        // main working tree is listed first, so this covers reusing a branch
        // that lives in the main tree as well as in a secondary worktree.
        let output = self
            .output(repo_root, &["worktree", "list", "--porcelain"])
            .await
            .map_err(delta_usecase::Error::from)?;
        if !output.status.success() {
            return Err(command_error("worktree list --porcelain", &output).into());
        }
        let listing = trimmed_stdout(&output);
        let target_ref = format!("refs/heads/{branch}");
        let mut current_path: Option<&str> = None;
        for line in listing.lines() {
            if let Some(path) = line.strip_prefix("worktree ") {
                current_path = Some(path);
            } else if let Some(ref_name) = line.strip_prefix("branch ") {
                if ref_name == target_ref {
                    return Ok(current_path.map(str::to_owned));
                }
            }
        }
        Ok(None)
    }

    async fn add_worktree_checkout(
        &self,
        repo_root: &str,
        worktree_path: &str,
        branch: &str,
    ) -> std::result::Result<(), delta_usecase::Error> {
        // Fetch so the branch reflects the latest remote tip before checkout.
        // For a real remote branch this succeeds; otherwise git's stderr is
        // surfaced rather than silently swallowed.
        self.run(
            repo_root,
            "fetch origin <branch>",
            &["fetch", REMOTE, branch],
        )
        .await
        .map_err(delta_usecase::Error::from)?;

        ensure_worktree_parent_dir(worktree_path)
            .await
            .map_err(delta_usecase::Error::from)?;

        // Does a local branch `<branch>` already exist? `show-ref --verify`
        // exits 0 when `refs/heads/<branch>` resolves, non-zero otherwise — the
        // expected "no such local branch" signal, not an error to propagate.
        let local_ref = format!("refs/heads/{branch}");
        let exists = self
            .output(repo_root, &["show-ref", "--verify", "--quiet", &local_ref])
            .await
            .map_err(delta_usecase::Error::from)?
            .status
            .success();

        if exists {
            // Check the existing local branch out in the new worktree.
            self.run(
                repo_root,
                "worktree add <branch>",
                &["worktree", "add", worktree_path, branch],
            )
            .await
            .map_err(delta_usecase::Error::from)
        } else {
            // No local branch yet: create one tracking `origin/<branch>` and
            // check it out in the new worktree.
            let start_ref = format!("{ORIGIN_PREFIX}{branch}");
            self.run(
                repo_root,
                "worktree add --track -b <branch>",
                &[
                    "worktree",
                    "add",
                    "--track",
                    "-b",
                    branch,
                    worktree_path,
                    &start_ref,
                ],
            )
            .await
            .map_err(delta_usecase::Error::from)
        }
    }

    async fn ensure_dir_trusted(&self, dir: &str) -> std::result::Result<(), delta_usecase::Error> {
        // Serialize Delta's own read-modify-write of the shared config across the
        // whole operation. The lock is process-local (delta-server is a single
        // process); see `trust` for why the residual delta-vs-claude race is
        // accepted rather than guarded with file locking.
        let _guard = self.trust_lock.lock().await;
        crate::trust::ensure_dir_trusted(&self.config_path, dir)
            .await
            .map_err(delta_usecase::Error::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Initialize a git repository at `dir` with one commit, so it has a `HEAD`
    /// to branch off, and a deterministic identity/branch name regardless of the
    /// host's git config.
    async fn init_repo_with_commit(dir: &std::path::Path) {
        let dir = dir.to_str().unwrap();
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Test"],
            vec!["commit", "-q", "--allow-empty", "-m", "initial"],
        ] {
            let status = Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(&args)
                .output()
                .await
                .expect("git available");
            assert!(
                status.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&status.stderr)
            );
        }
    }

    #[tokio::test]
    async fn repo_root_reports_the_toplevel_for_a_git_dir() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo_with_commit(tmp.path()).await;
        let git = Git::new();

        let root = git
            .repo_root(tmp.path().to_str().unwrap())
            .await
            .unwrap()
            .expect("a git repo reports a root");

        // Compare canonicalized: git may print a symlink-resolved path (macOS
        // `/var` → `/private/var`).
        let expected = tokio::fs::canonicalize(tmp.path())
            .await
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let actual = tokio::fs::canonicalize(&root)
            .await
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert_eq!(actual, expected);
    }

    #[tokio::test]
    async fn current_branch_reports_the_local_branch_name() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo_with_commit(tmp.path()).await;
        let git = Git::new();

        let branch = git
            .current_branch(tmp.path().to_str().unwrap())
            .await
            .unwrap()
            .expect("a fresh repo on its initial branch reports a branch");
        assert_eq!(branch, "main");
    }

    #[tokio::test]
    async fn current_branch_is_none_outside_a_git_repo() {
        // A bare temp directory with no git repo above it.
        let tmp = tempfile::tempdir().unwrap();
        let git = Git::new();

        let branch = git
            .current_branch(tmp.path().to_str().unwrap())
            .await
            .unwrap();
        assert!(branch.is_none(), "a non-git directory has no branch");
    }

    #[tokio::test]
    async fn current_branch_is_none_when_head_is_detached() {
        // A detached HEAD has no branch name to record at launch time;
        // `rev-parse --abbrev-ref HEAD` returns the literal `HEAD`, which the
        // gateway translates to `None`.
        let tmp = tempfile::tempdir().unwrap();
        init_repo_with_commit(tmp.path()).await;
        let repo = tmp.path().to_str().unwrap();
        // Detach HEAD onto the initial commit so HEAD no longer points at a
        // branch ref.
        let status = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(["checkout", "--detach", "HEAD"])
            .output()
            .await
            .unwrap();
        assert!(
            status.status.success(),
            "detach failed: {}",
            String::from_utf8_lossy(&status.stderr)
        );
        let git = Git::new();

        let branch = git.current_branch(repo).await.unwrap();
        assert!(branch.is_none(), "a detached HEAD reports no branch");
    }

    #[tokio::test]
    async fn repo_root_is_none_outside_a_git_repo() {
        // A bare temp directory with no git repo above it.
        let tmp = tempfile::tempdir().unwrap();
        let git = Git::new();

        let root = git.repo_root(tmp.path().to_str().unwrap()).await.unwrap();
        assert!(root.is_none(), "a non-git directory has no repo root");
    }

    #[tokio::test]
    async fn create_worktree_from_head_produces_a_worktree_and_branch() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo_with_commit(tmp.path()).await;
        let repo_root = tmp.path().to_str().unwrap().to_owned();
        let git = Git::new();

        // Create the worktree outside the repo dir so it is a sibling path.
        let worktree_dir = tempfile::tempdir().unwrap();
        let worktree_path = worktree_dir
            .path()
            .join("delta-session-1")
            .to_string_lossy()
            .into_owned();

        git.create_worktree(
            &repo_root,
            &worktree_path,
            "delta-session-1",
            WorktreeStartPoint::Head,
        )
        .await
        .unwrap();

        // The worktree directory exists and is itself a git working tree whose
        // root is the worktree path.
        assert!(
            tokio::fs::metadata(&worktree_path).await.unwrap().is_dir(),
            "the worktree directory was created"
        );
        let wt_root = git
            .repo_root(&worktree_path)
            .await
            .unwrap()
            .expect("the worktree is a git working tree");
        let expected = tokio::fs::canonicalize(&worktree_path)
            .await
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let actual = tokio::fs::canonicalize(&wt_root)
            .await
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert_eq!(actual, expected);

        // The new branch is checked out in the worktree.
        let head = Command::new("git")
            .arg("-C")
            .arg(&worktree_path)
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .output()
            .await
            .unwrap();
        assert_eq!(trimmed_stdout(&head), "delta-session-1");
    }

    #[tokio::test]
    async fn create_worktree_creates_the_missing_parent_directory() {
        // The worktree base (e.g. `$HOME/.delta/worktrees`) may not exist yet on
        // a fresh install; `git worktree add` would fail if its parent is
        // absent. The gateway must create the parent chain first.
        let tmp = tempfile::tempdir().unwrap();
        init_repo_with_commit(tmp.path()).await;
        let repo_root = tmp.path().to_str().unwrap().to_owned();
        let git = Git::new();

        // A worktree path two levels under a base that does not exist yet.
        let base = tempfile::tempdir().unwrap();
        let missing_base = base.path().join("does/not/exist/yet");
        assert!(
            !missing_base.exists(),
            "the worktree base does not exist before the call"
        );
        let worktree_path = missing_base
            .join("delta-session-1")
            .to_string_lossy()
            .into_owned();

        git.create_worktree(
            &repo_root,
            &worktree_path,
            "delta-session-1",
            WorktreeStartPoint::Head,
        )
        .await
        .expect("the missing parent is created so the worktree add succeeds");

        assert!(
            tokio::fs::metadata(&worktree_path).await.unwrap().is_dir(),
            "the worktree directory exists under the freshly-created base"
        );
    }

    #[tokio::test]
    async fn create_worktree_surfaces_git_stderr_on_failure() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo_with_commit(tmp.path()).await;
        let repo_root = tmp.path().to_str().unwrap().to_owned();
        let git = Git::new();

        let worktree_dir = tempfile::tempdir().unwrap();
        let worktree_path = worktree_dir
            .path()
            .join("wt")
            .to_string_lossy()
            .into_owned();

        // `main` already exists, so `worktree add -b main` must fail; the error
        // surfaces git's stderr rather than a generic message.
        let err = git
            .create_worktree(&repo_root, &worktree_path, "main", WorktreeStartPoint::Head)
            .await
            .unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains("git error:"),
            "a worktree-add failure maps to a git error, got: {message}"
        );
    }

    /// A local "remote": a second clone whose `origin` is the first repo, so
    /// fetch/`origin/<branch>` work fully offline.
    #[tokio::test]
    async fn fetch_remote_branches_lists_origin_branches_offline() {
        // Origin repo with a `main` and a `feature` branch.
        let origin = tempfile::tempdir().unwrap();
        init_repo_with_commit(origin.path()).await;
        let origin_path = origin.path().to_str().unwrap();
        let status = Command::new("git")
            .arg("-C")
            .arg(origin_path)
            .args(["branch", "feature"])
            .output()
            .await
            .unwrap();
        assert!(status.status.success());

        // Clone it so the clone has `origin` pointing at the first repo and an
        // `origin/HEAD` symref recording the default branch.
        let clone_parent = tempfile::tempdir().unwrap();
        let clone_path = clone_parent
            .path()
            .join("clone")
            .to_string_lossy()
            .into_owned();
        let status = Command::new("git")
            .args(["clone", "-q", origin_path, &clone_path])
            .output()
            .await
            .unwrap();
        assert!(
            status.status.success(),
            "clone failed: {}",
            String::from_utf8_lossy(&status.stderr)
        );

        let git = Git::new();
        let remote = git.fetch_remote_branches(&clone_path).await.unwrap();

        assert!(
            remote.branches.contains(&"main".to_owned()),
            "main is a remote branch, got {:?}",
            remote.branches
        );
        assert!(
            remote.branches.contains(&"feature".to_owned()),
            "feature is a remote branch, got {:?}",
            remote.branches
        );
        assert!(
            !remote.branches.iter().any(|b| b == "HEAD"),
            "the origin/HEAD symref is excluded, got {:?}",
            remote.branches
        );
        assert_eq!(
            remote.default_branch.as_deref(),
            Some("main"),
            "the clone's origin/HEAD records main as the default branch"
        );

        // A worktree off the remote `feature` branch starts from origin/feature.
        let worktree_dir = tempfile::tempdir().unwrap();
        let worktree_path = worktree_dir
            .path()
            .join("delta-remote")
            .to_string_lossy()
            .into_owned();
        git.create_worktree(
            &clone_path,
            &worktree_path,
            "delta-remote",
            WorktreeStartPoint::RemoteBranch("feature".to_owned()),
        )
        .await
        .unwrap();
        assert!(
            tokio::fs::metadata(&worktree_path).await.unwrap().is_dir(),
            "a remote-branch worktree was created"
        );
    }

    /// Run `git -C <dir> <args>`, asserting success, returning trimmed stdout.
    async fn git_ok(dir: &str, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .await
            .expect("git available");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        trimmed_stdout(&output)
    }

    /// Canonicalize a path so comparisons survive symlink resolution (macOS
    /// `/var` → `/private/var`).
    async fn canonical(path: &str) -> String {
        tokio::fs::canonicalize(path)
            .await
            .unwrap()
            .to_string_lossy()
            .into_owned()
    }

    /// Clone `origin_path` into a fresh temp dir, returning the clone path.
    async fn clone_into(origin_path: &str, parent: &std::path::Path) -> String {
        let clone_path = parent.join("clone").to_string_lossy().into_owned();
        let status = Command::new("git")
            .args(["clone", "-q", origin_path, &clone_path])
            .output()
            .await
            .unwrap();
        assert!(
            status.status.success(),
            "clone failed: {}",
            String::from_utf8_lossy(&status.stderr)
        );
        clone_path
    }

    #[tokio::test]
    async fn worktree_path_for_branch_finds_a_secondary_worktree() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo_with_commit(tmp.path()).await;
        let repo_root = tmp.path().to_str().unwrap().to_owned();
        let git = Git::new();

        // Check `feature` out in a secondary worktree.
        let wt_dir = tempfile::tempdir().unwrap();
        let wt_path = wt_dir
            .path()
            .join("feature-wt")
            .to_string_lossy()
            .into_owned();
        git_ok(&repo_root, &["worktree", "add", "-b", "feature", &wt_path]).await;

        let found = git
            .worktree_path_for_branch(&repo_root, "feature")
            .await
            .unwrap()
            .expect("feature is checked out in the secondary worktree");
        assert_eq!(canonical(&found).await, canonical(&wt_path).await);
    }

    #[tokio::test]
    async fn worktree_path_for_branch_finds_the_main_working_tree() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo_with_commit(tmp.path()).await;
        let repo_root = tmp.path().to_str().unwrap().to_owned();
        let git = Git::new();

        // `main` is checked out in the main working tree (the repo root itself).
        let found = git
            .worktree_path_for_branch(&repo_root, "main")
            .await
            .unwrap()
            .expect("main is the main working tree's branch");
        assert_eq!(canonical(&found).await, canonical(&repo_root).await);
    }

    #[tokio::test]
    async fn worktree_path_for_branch_is_none_when_not_checked_out() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo_with_commit(tmp.path()).await;
        let repo_root = tmp.path().to_str().unwrap().to_owned();
        // A local branch that exists but is not checked out anywhere.
        git_ok(&repo_root, &["branch", "idle"]).await;
        let git = Git::new();

        let found = git
            .worktree_path_for_branch(&repo_root, "idle")
            .await
            .unwrap();
        assert!(
            found.is_none(),
            "an unchecked-out branch has no worktree path"
        );
    }

    #[tokio::test]
    async fn add_worktree_checkout_uses_an_existing_local_branch() {
        // Origin with `main` and `feature`; the clone fetches both.
        let origin = tempfile::tempdir().unwrap();
        init_repo_with_commit(origin.path()).await;
        let origin_path = origin.path().to_str().unwrap();
        git_ok(origin_path, &["branch", "feature"]).await;

        let clone_parent = tempfile::tempdir().unwrap();
        let clone_path = clone_into(origin_path, clone_parent.path()).await;
        // Create the local branch so the "existing local branch" path is taken.
        git_ok(&clone_path, &["branch", "feature", "origin/feature"]).await;

        let git = Git::new();
        let wt_dir = tempfile::tempdir().unwrap();
        let wt_path = wt_dir
            .path()
            .join("feature-wt")
            .to_string_lossy()
            .into_owned();

        git.add_worktree_checkout(&clone_path, &wt_path, "feature")
            .await
            .unwrap();

        // The worktree checked out `feature` itself (not a `delta-<id>` branch).
        let head = git_ok(&wt_path, &["rev-parse", "--abbrev-ref", "HEAD"]).await;
        assert_eq!(head, "feature");
    }

    #[tokio::test]
    async fn add_worktree_checkout_creates_a_tracking_branch_from_origin() {
        // Origin with a `feature` branch the clone has not checked out locally.
        let origin = tempfile::tempdir().unwrap();
        init_repo_with_commit(origin.path()).await;
        let origin_path = origin.path().to_str().unwrap();
        git_ok(origin_path, &["branch", "feature"]).await;

        let clone_parent = tempfile::tempdir().unwrap();
        let clone_path = clone_into(origin_path, clone_parent.path()).await;
        // No local `feature` branch yet: the tracking-branch path is taken.

        let git = Git::new();
        let wt_dir = tempfile::tempdir().unwrap();
        let wt_path = wt_dir
            .path()
            .join("feature-wt")
            .to_string_lossy()
            .into_owned();

        git.add_worktree_checkout(&clone_path, &wt_path, "feature")
            .await
            .unwrap();

        // The worktree is on a local `feature` branch tracking `origin/feature`.
        let head = git_ok(&wt_path, &["rev-parse", "--abbrev-ref", "HEAD"]).await;
        assert_eq!(head, "feature");
        let upstream = git_ok(
            &wt_path,
            &[
                "rev-parse",
                "--abbrev-ref",
                "--symbolic-full-name",
                "@{upstream}",
            ],
        )
        .await;
        assert_eq!(upstream, "origin/feature");
    }
}
