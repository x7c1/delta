//! Commits and tree comparisons: which commit a worktree start point resolves
//! to, which commit a working tree is on, and what changed between two
//! commits.
//!
//! These back the launch-trust rule (see `delta_usecase`'s lifecycle module):
//! the use case compares the commit a worktree will check out with the commit
//! the user trusts, reading everything from git objects so nothing in a
//! not-yet-built (or just-built) worktree is ever read as a file.

use delta_usecase::WorktreeStartPoint;

use super::{command_error, trimmed_stdout, Git, ORIGIN_PREFIX, REMOTE};
use crate::error::Error;

impl Git {
    /// The commit id `rev` names in the repository at `repo`, peeled to a
    /// commit. `--verify` makes git print exactly one id or fail, and
    /// `--end-of-options` keeps `rev` from ever being read as a flag.
    async fn commit_of(&self, repo: &str, rev: &str) -> Result<String, Error> {
        let peeled = format!("{rev}^{{commit}}");
        let output = self
            .output(
                repo,
                &["rev-parse", "--verify", "--end-of-options", &peeled],
            )
            .await?;
        if !output.status.success() {
            return Err(command_error("rev-parse --verify <rev>", &output));
        }
        Ok(trimmed_stdout(&output))
    }

    /// The commit `HEAD` is on in the working tree at `dir`.
    pub(super) async fn head_commit_of(&self, dir: &str) -> Result<String, Error> {
        self.commit_of(dir, "HEAD").await
    }

    /// The commit a worktree built from `start_point` would check out, after
    /// the same fetch the build itself runs first.
    ///
    /// Mirrors the build exactly: `Head` is the current `HEAD`;
    /// `RemoteBranch(name)` is the fetched `origin/<name>`; `UseRemoteBranch`
    /// checks out the local branch `<name>` when one exists (and reuses the
    /// worktree already holding it), otherwise a new branch at the fetched
    /// `origin/<name>`.
    pub(super) async fn launch_commit_of(
        &self,
        repo_root: &str,
        start_point: &WorktreeStartPoint,
    ) -> Result<String, Error> {
        match start_point {
            WorktreeStartPoint::Head => self.commit_of(repo_root, "HEAD").await,
            WorktreeStartPoint::RemoteBranch(name) => {
                self.run(repo_root, "fetch origin <branch>", &["fetch", REMOTE, name])
                    .await?;
                self.commit_of(repo_root, &format!("refs/remotes/{ORIGIN_PREFIX}{name}"))
                    .await
            }
            WorktreeStartPoint::UseRemoteBranch(name) => {
                self.run(repo_root, "fetch origin <branch>", &["fetch", REMOTE, name])
                    .await?;
                let local_ref = format!("refs/heads/{name}");
                let local_exists = self
                    .output(repo_root, &["show-ref", "--verify", "--quiet", &local_ref])
                    .await?
                    .status
                    .success();
                if local_exists {
                    self.commit_of(repo_root, &local_ref).await
                } else {
                    self.commit_of(repo_root, &format!("refs/remotes/{ORIGIN_PREFIX}{name}"))
                        .await
                }
            }
        }
    }

    /// Every path whose content differs between the trees of commits `from`
    /// and `to`, as `git diff --name-only` reports it.
    ///
    /// `-z` keeps unusual file names intact (no quoting), and `--no-renames`
    /// reports a rename as its two sides, so a file moved into a watched
    /// location is listed under its new name.
    pub(super) async fn changed_paths_between(
        &self,
        repo_root: &str,
        from: &str,
        to: &str,
    ) -> Result<Vec<String>, Error> {
        let output = self
            .output(
                repo_root,
                &[
                    "diff",
                    "--name-only",
                    "-z",
                    "--no-renames",
                    "--end-of-options",
                    from,
                    to,
                ],
            )
            .await?;
        if !output.status.success() {
            return Err(command_error("diff --name-only", &output));
        }
        Ok(output
            .stdout
            .split(|byte| *byte == 0)
            .filter(|name| !name.is_empty())
            .map(|name| String::from_utf8_lossy(name).into_owned())
            .collect())
    }

    /// The unified diff of one `path` between the trees of commits `from` and
    /// `to`.
    ///
    /// The path is matched literally (`:(top,literal)`), so a name containing
    /// glob characters names only itself. External diff drivers and textconv
    /// filters are switched off: the text shown is git's own rendering of the
    /// two blobs, never the output of a program the repository configured.
    pub(super) async fn file_diff_between(
        &self,
        repo_root: &str,
        from: &str,
        to: &str,
        path: &str,
    ) -> Result<String, Error> {
        let pathspec = format!(":(top,literal){path}");
        let output = self
            .output(
                repo_root,
                &[
                    "diff",
                    "--no-color",
                    "--no-ext-diff",
                    "--no-textconv",
                    "--no-renames",
                    "--end-of-options",
                    from,
                    to,
                    "--",
                    &pathspec,
                ],
            )
            .await?;
        if !output.status.success() {
            return Err(command_error("diff <path>", &output));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}
