//! [`Git`]: the concrete [`GitWorktree`].
//!
//! Split by responsibility: this module holds the struct, the shared `git`
//! invocation helpers and the [`GitWorktree`] trait wiring, `removal` the
//! worktree removal and branch deletion, `inspection` what git says about
//! a directory as a worktree, and `commits` the commit lookups and tree
//! comparisons the launch-trust rule reads.

mod commits;
mod inspection;
mod removal;
#[cfg(test)]
mod testing;
#[cfg(test)]
mod tests;

use std::path::PathBuf;

use async_trait::async_trait;
use tokio::process::Command;
use tokio::sync::Mutex;

use delta_usecase::{
    BranchDeletion, GitWorktree, RemoteBranches, WorktreeInspection, WorktreeRemoval,
    WorktreeStartPoint,
};

use crate::error::Error;

/// The `origin/` prefix `git` uses for remote-tracking refs, stripped to return
/// branch short names.
const ORIGIN_PREFIX: &str = "origin/";

/// The remote name Delta operates against. Worktrees branch off `origin/<name>`
/// and the default branch is read from `origin/HEAD`.
const REMOTE: &str = "origin";

/// Detects git repositories, and creates and removes per-session git
/// worktrees, by shelling out to `git`.
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
    /// Environment variables set on every `git` command, on top of the
    /// inherited environment (the server configuration's `child_env`). `git`
    /// itself is also looked up on this `PATH` when it carries one.
    env: Vec<(String, String)>,
}

impl Git {
    /// Create a new git worktree gateway running every `git` command with `env`
    /// set, defaulting the trust-config path to `$HOME/.claude.json` (falling
    /// back to `.claude.json` in the current directory if `HOME` is unset,
    /// which only happens in degenerate environments).
    pub fn new(env: Vec<(String, String)>) -> Self {
        let config_path = match std::env::var_os("HOME") {
            Some(home) => PathBuf::from(home).join(".claude.json"),
            None => PathBuf::from(".claude.json"),
        };
        Self::with_config_path(config_path, env)
    }

    /// Create a gateway with an explicit trust-config path. Used by tests to
    /// target a temp file instead of the real `~/.claude.json`.
    pub fn with_config_path(config_path: PathBuf, env: Vec<(String, String)>) -> Self {
        Self {
            config_path,
            trust_lock: Mutex::new(()),
            env,
        }
    }

    /// The command `git -C <repo>`, with [`Self::env`] set. Every `git` this
    /// gateway runs starts here, so none can miss the environment.
    fn git(&self, repo: &str) -> Command {
        let mut command = Command::new("git");
        command
            .envs(self.env.iter().map(|(name, value)| (name, value)))
            .arg("-C")
            .arg(repo);
        command
    }

    /// Run `git -C <repo> <args>`, returning the captured output.
    async fn output(
        &self,
        repo: &str,
        args: &[&str],
    ) -> std::result::Result<std::process::Output, Error> {
        Ok(self.git(repo).args(args).output().await?)
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

    async fn origin_url(
        &self,
        path: &str,
    ) -> std::result::Result<Option<String>, delta_usecase::Error> {
        // `config --get remote.origin.url` prints the configured URL and exits
        // 0; it exits non-zero when the key is unset OR `path` is not a git
        // repo. Both are the expected `None` signal, not errors to propagate.
        // `remote.origin.url` lives in the shared `.git/config`, so a linked
        // worktree returns the same URL as the main tree — repository identity
        // is stable across worktrees.
        let output = self
            .output(path, &["config", "--get", "remote.origin.url"])
            .await
            .map_err(delta_usecase::Error::from)?;
        if !output.status.success() {
            return Ok(None);
        }
        let url = trimmed_stdout(&output);
        if url.is_empty() {
            Ok(None)
        } else {
            Ok(Some(url))
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

    async fn is_dir_trusted(&self, dir: &str) -> std::result::Result<bool, delta_usecase::Error> {
        // Read under the same lock as the writes, so a read never lands
        // between Delta's own temp-file write and its rename.
        let _guard = self.trust_lock.lock().await;
        crate::trust::is_dir_trusted(&self.config_path, dir)
            .await
            .map_err(delta_usecase::Error::from)
    }

    async fn record_dir_trust(&self, dir: &str) -> std::result::Result<(), delta_usecase::Error> {
        let _guard = self.trust_lock.lock().await;
        crate::trust::record_dir_trust(&self.config_path, dir)
            .await
            .map_err(delta_usecase::Error::from)
    }

    async fn launch_commit(
        &self,
        repo_root: &str,
        start_point: &WorktreeStartPoint,
    ) -> std::result::Result<String, delta_usecase::Error> {
        self.launch_commit_of(repo_root, start_point)
            .await
            .map_err(delta_usecase::Error::from)
    }

    async fn head_commit(&self, dir: &str) -> std::result::Result<String, delta_usecase::Error> {
        self.head_commit_of(dir)
            .await
            .map_err(delta_usecase::Error::from)
    }

    async fn changed_paths(
        &self,
        repo_root: &str,
        from: &str,
        to: &str,
    ) -> std::result::Result<Vec<String>, delta_usecase::Error> {
        self.changed_paths_between(repo_root, from, to)
            .await
            .map_err(delta_usecase::Error::from)
    }

    async fn file_diff(
        &self,
        repo_root: &str,
        from: &str,
        to: &str,
        path: &str,
    ) -> std::result::Result<String, delta_usecase::Error> {
        self.file_diff_between(repo_root, from, to, path)
            .await
            .map_err(delta_usecase::Error::from)
    }

    async fn remove_worktree(
        &self,
        repo_root: &str,
        path: &str,
    ) -> std::result::Result<WorktreeRemoval, delta_usecase::Error> {
        self.remove_clean_worktree(repo_root, path)
            .await
            .map_err(delta_usecase::Error::from)
    }

    async fn delete_branch_if_merged(
        &self,
        repo_root: &str,
        branch: &str,
    ) -> std::result::Result<BranchDeletion, delta_usecase::Error> {
        self.delete_merged_branch(repo_root, branch)
            .await
            .map_err(delta_usecase::Error::from)
    }

    async fn is_branch_merged(
        &self,
        repo_root: &str,
        branch: &str,
    ) -> std::result::Result<Option<bool>, delta_usecase::Error> {
        self.branch_is_merged(repo_root, branch)
            .await
            .map_err(delta_usecase::Error::from)
    }

    async fn prune_worktrees(
        &self,
        repo_root: &str,
    ) -> std::result::Result<(), delta_usecase::Error> {
        self.run(repo_root, "worktree prune", &["worktree", "prune"])
            .await
            .map_err(delta_usecase::Error::from)
    }

    async fn forget_dir_trusted(&self, dir: &str) -> std::result::Result<(), delta_usecase::Error> {
        // The same process-local lock as `ensure_dir_trusted`: both are a
        // read-modify-write of the one config file.
        let _guard = self.trust_lock.lock().await;
        crate::trust::forget_dir_trusted(&self.config_path, dir)
            .await
            .map_err(delta_usecase::Error::from)
    }

    async fn inspect_worktree(
        &self,
        path: &str,
    ) -> std::result::Result<Option<WorktreeInspection>, delta_usecase::Error> {
        self.inspect(path).await.map_err(delta_usecase::Error::from)
    }

    async fn force_remove_worktree(
        &self,
        repo_root: &str,
        path: &str,
    ) -> std::result::Result<(), delta_usecase::Error> {
        self.remove_worktree_forcibly(repo_root, path)
            .await
            .map_err(delta_usecase::Error::from)
    }
}
