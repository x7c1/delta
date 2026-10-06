//! Removal of a session's worktree and its branch, which keeps whatever git
//! refuses to remove (a worktree with modified or untracked files, an unmerged
//! branch).

use delta_usecase::{BranchDeletion, WorktreeRemoval};
use tokio::process::Command;

use super::{command_error, Git};
use crate::error::Error;

/// What `git worktree remove` (without `--force`) says when it refuses a
/// worktree with modified or untracked files. Matched against git's stderr
/// under the C locale ([`Git::untranslated_output`]).
const DIRTY_WORKTREE_REFUSAL: &str = "contains modified or untracked files";

/// What `git branch -d` says when it refuses a branch that is not merged.
/// Older gits capitalize the sentence (`The branch ...`), so this is matched
/// on the part they share, under the C locale.
const UNMERGED_BRANCH_REFUSAL: &str = "is not fully merged";

impl Git {
    /// Run `git -C <repo> <args>` under the C locale, returning the captured
    /// output — for a command whose refusal is told apart from other failures
    /// by git's message, which a translated git would word differently.
    async fn untranslated_output(
        &self,
        repo: &str,
        args: &[&str],
    ) -> std::result::Result<std::process::Output, Error> {
        Ok(Command::new("git")
            .env("LC_ALL", "C")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .await?)
    }

    /// `git worktree remove <path>`, never `--force`: git's own refusal of a
    /// worktree with modified or untracked files is what keeps the user's work.
    /// Any other non-zero exit (a locked worktree, a path that is not a
    /// worktree, a repository that is gone) is a real error.
    pub(super) async fn remove_clean_worktree(
        &self,
        repo_root: &str,
        path: &str,
    ) -> std::result::Result<WorktreeRemoval, Error> {
        let output = self
            .untranslated_output(repo_root, &["worktree", "remove", path])
            .await?;
        if output.status.success() {
            Ok(WorktreeRemoval::Removed)
        } else if refused_with(&output, DIRTY_WORKTREE_REFUSAL) {
            Ok(WorktreeRemoval::KeptDirty)
        } else {
            Err(command_error("worktree remove", &output))
        }
    }

    /// `git branch -d <branch>`, never `-D`: git's refusal of an unmerged
    /// branch keeps its commits.
    pub(super) async fn delete_merged_branch(
        &self,
        repo_root: &str,
        branch: &str,
    ) -> std::result::Result<BranchDeletion, Error> {
        // Tell "no such branch" apart up front rather than by parsing
        // `branch -d`'s message: `show-ref --verify` exits non-zero exactly
        // when `refs/heads/<branch>` does not resolve.
        let local_ref = format!("refs/heads/{branch}");
        let exists = self
            .output(repo_root, &["show-ref", "--verify", "--quiet", &local_ref])
            .await?
            .status
            .success();
        if !exists {
            return Ok(BranchDeletion::Absent);
        }
        let output = self
            .untranslated_output(repo_root, &["branch", "-d", branch])
            .await?;
        if output.status.success() {
            Ok(BranchDeletion::Deleted)
        } else if refused_with(&output, UNMERGED_BRANCH_REFUSAL) {
            Ok(BranchDeletion::KeptUnmerged)
        } else {
            Err(command_error("branch -d", &output))
        }
    }
}

/// True iff `output` is a non-zero exit whose stderr carries `refusal`.
fn refused_with(output: &std::process::Output, refusal: &str) -> bool {
    !output.status.success() && String::from_utf8_lossy(&output.stderr).contains(refusal)
}

#[cfg(test)]
mod tests {
    use delta_usecase::GitWorktree;

    use super::*;
    use crate::git::testing::{canonical, git_ok, init_repo_with_commit};

    /// A repository at a fresh temp dir with one commit and a linked worktree
    /// on a new branch `branch` at `<worktrees>/wt`. Returns the repo root and
    /// the worktree path; keep the two temp dirs alive for the test.
    async fn repo_with_worktree(
        repo: &tempfile::TempDir,
        worktrees: &tempfile::TempDir,
        branch: &str,
    ) -> (String, String) {
        init_repo_with_commit(repo.path()).await;
        let repo_root = repo.path().to_str().unwrap().to_owned();
        let wt_path = worktrees.path().join("wt").to_string_lossy().into_owned();
        git_ok(
            &repo_root,
            &["worktree", "add", "-q", "-b", branch, &wt_path],
        )
        .await;
        (repo_root, wt_path)
    }

    /// The canonicalized paths `git worktree list --porcelain` reports.
    async fn listed_worktrees(repo_root: &str) -> Vec<String> {
        let listing = git_ok(repo_root, &["worktree", "list", "--porcelain"]).await;
        let mut paths = Vec::new();
        for line in listing.lines() {
            if let Some(path) = line.strip_prefix("worktree ") {
                paths.push(canonical(path).await);
            }
        }
        paths
    }

    #[tokio::test]
    async fn remove_worktree_removes_a_clean_worktree() {
        let (repo, worktrees) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (repo_root, wt_path) = repo_with_worktree(&repo, &worktrees, "delta-s1").await;
        let wt_canonical = canonical(&wt_path).await;
        let git = Git::new();

        let outcome = git.remove_worktree(&repo_root, &wt_path).await.unwrap();

        assert_eq!(outcome, WorktreeRemoval::Removed);
        assert!(
            !std::path::Path::new(&wt_path).exists(),
            "the directory is gone"
        );
        assert!(
            !listed_worktrees(&repo_root).await.contains(&wt_canonical),
            "git worktree list no longer shows it"
        );
        git.prune_worktrees(&repo_root)
            .await
            .expect("pruning after a removal succeeds");
    }

    #[tokio::test]
    async fn remove_worktree_keeps_a_worktree_with_an_untracked_file() {
        let (repo, worktrees) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (repo_root, wt_path) = repo_with_worktree(&repo, &worktrees, "delta-s1").await;
        let untracked = std::path::Path::new(&wt_path).join("notes.txt");
        tokio::fs::write(&untracked, "work in progress")
            .await
            .unwrap();
        let git = Git::new();

        let outcome = git.remove_worktree(&repo_root, &wt_path).await.unwrap();

        assert_eq!(
            outcome,
            WorktreeRemoval::KeptDirty,
            "git's refusal is reported as kept"
        );
        assert!(untracked.exists(), "the untracked file is still there");
        assert!(
            listed_worktrees(&repo_root)
                .await
                .contains(&canonical(&wt_path).await),
            "the worktree is still registered"
        );
    }

    #[tokio::test]
    async fn remove_worktree_of_an_unknown_path_is_an_error_not_a_refusal() {
        let (repo, worktrees) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (repo_root, _) = repo_with_worktree(&repo, &worktrees, "delta-s1").await;
        let stray = tempfile::tempdir().unwrap();
        let git = Git::new();

        let result = git
            .remove_worktree(&repo_root, stray.path().to_str().unwrap())
            .await;

        assert!(
            result.is_err(),
            "a path that is not a worktree is a failure, got {result:?}"
        );
        assert!(stray.path().exists(), "and nothing is deleted");
    }

    #[tokio::test]
    async fn delete_branch_if_merged_keeps_an_unmerged_branch() {
        let (repo, worktrees) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (repo_root, wt_path) = repo_with_worktree(&repo, &worktrees, "delta-s1").await;
        // Commit on the branch, so it is ahead of `main` (its merge target: it
        // has no upstream), then drop the worktree so git does not refuse the
        // deletion for the branch being checked out.
        git_ok(&wt_path, &["commit", "-q", "--allow-empty", "-m", "work"]).await;
        git_ok(&repo_root, &["worktree", "remove", &wt_path]).await;
        let git = Git::new();

        let outcome = git
            .delete_branch_if_merged(&repo_root, "delta-s1")
            .await
            .unwrap();

        assert_eq!(outcome, BranchDeletion::KeptUnmerged);
        git_ok(&repo_root, &["show-ref", "--verify", "refs/heads/delta-s1"]).await;
    }

    #[tokio::test]
    async fn delete_branch_if_merged_deletes_a_merged_branch_and_reports_an_absent_one() {
        let (repo, worktrees) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (repo_root, wt_path) = repo_with_worktree(&repo, &worktrees, "delta-s1").await;
        git_ok(&repo_root, &["worktree", "remove", &wt_path]).await;
        let git = Git::new();

        let deleted = git
            .delete_branch_if_merged(&repo_root, "delta-s1")
            .await
            .unwrap();
        let again = git
            .delete_branch_if_merged(&repo_root, "delta-s1")
            .await
            .unwrap();

        assert_eq!(
            deleted,
            BranchDeletion::Deleted,
            "a branch with no new commits is merged"
        );
        assert_eq!(again, BranchDeletion::Absent);
    }
}
