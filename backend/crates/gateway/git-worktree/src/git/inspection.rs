//! What git says about a directory as a linked worktree: the repository it
//! belongs to, and whether it holds uncommitted work.

use std::path::Path;

use delta_usecase::WorktreeInspection;

use super::{command_error, trimmed_stdout, Git};
use crate::error::Error;

/// The `git worktree list --porcelain` line that opens each worktree's
/// stanza; the first stanza is always the main working tree.
const WORKTREE_LINE_PREFIX: &str = "worktree ";

impl Git {
    /// The repository and dirtiness of the worktree at `path`, or `None` when
    /// git does not know `path` as a working tree of its own (see
    /// [`delta_usecase::GitWorktree::inspect_worktree`]).
    pub(super) async fn inspect(&self, path: &str) -> Result<Option<WorktreeInspection>, Error> {
        // A directory whose repository forgot it (pruned) or is gone fails
        // here, as does one that is not a working tree at all: all "unknown".
        let toplevel = self.output(path, &["rev-parse", "--show-toplevel"]).await?;
        if !toplevel.status.success() {
            return Ok(None);
        }
        // A directory that merely lies inside some other working tree is not
        // a worktree of its own.
        if !same_dir(&trimmed_stdout(&toplevel), path).await {
            return Ok(None);
        }
        let listing = self
            .output(path, &["worktree", "list", "--porcelain"])
            .await?;
        if !listing.status.success() {
            return Err(command_error("worktree list", &listing));
        }
        let listing = String::from_utf8_lossy(&listing.stdout);
        let Some(repo_root) = listing
            .lines()
            .find_map(|line| line.strip_prefix(WORKTREE_LINE_PREFIX))
        else {
            return Ok(None);
        };
        let status = self.output(path, &["status", "--porcelain"]).await?;
        if !status.status.success() {
            return Err(command_error("status", &status));
        }
        Ok(Some(WorktreeInspection {
            repo_root: repo_root.to_owned(),
            dirty: !status.stdout.iter().all(u8::is_ascii_whitespace),
        }))
    }
}

/// Whether `a` and `b` name the same directory once symlinks are resolved
/// (git reports `/private/var/...` for a `/var/...` path on macOS).
async fn same_dir(a: &str, b: &str) -> bool {
    match (
        tokio::fs::canonicalize(Path::new(a)).await,
        tokio::fs::canonicalize(Path::new(b)).await,
    ) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use delta_usecase::GitWorktree;

    use crate::git::testing::{canonical, git_ok, init_repo_with_commit};
    use crate::git::Git;

    /// A repository with one commit and a linked worktree at `<worktrees>/wt`.
    async fn repo_with_worktree(
        repo: &tempfile::TempDir,
        worktrees: &tempfile::TempDir,
    ) -> (String, String) {
        init_repo_with_commit(repo.path()).await;
        let repo_root = repo.path().to_str().unwrap().to_owned();
        let wt_path = worktrees.path().join("wt").to_string_lossy().into_owned();
        git_ok(
            &repo_root,
            &["worktree", "add", "-q", "-b", "delta-s1", &wt_path],
        )
        .await;
        (repo_root, wt_path)
    }

    #[tokio::test]
    async fn a_clean_worktree_reports_its_repository_and_no_work() {
        let (repo, worktrees) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (repo_root, wt_path) = repo_with_worktree(&repo, &worktrees).await;

        let inspection = Git::new()
            .inspect_worktree(&wt_path)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(
            canonical(&inspection.repo_root).await,
            canonical(&repo_root).await
        );
        assert!(!inspection.dirty);
    }

    #[tokio::test]
    async fn an_untracked_or_modified_file_makes_the_worktree_dirty() {
        let (repo, worktrees) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (_, wt_path) = repo_with_worktree(&repo, &worktrees).await;
        std::fs::write(std::path::Path::new(&wt_path).join("notes.txt"), "work").unwrap();

        let inspection = Git::new()
            .inspect_worktree(&wt_path)
            .await
            .unwrap()
            .unwrap();

        assert!(inspection.dirty);
    }

    #[tokio::test]
    async fn a_pruned_worktree_or_a_plain_directory_is_unknown_to_git() {
        let (repo, worktrees) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (repo_root, wt_path) = repo_with_worktree(&repo, &worktrees).await;
        // Drop the repository's administrative entry while the directory stays
        // behind — what a `git worktree prune` after a move leaves.
        let admin = std::path::Path::new(&repo_root).join(".git/worktrees/wt");
        std::fs::remove_dir_all(admin).unwrap();
        let plain = tempfile::tempdir().unwrap();
        let git = Git::new();

        assert_eq!(git.inspect_worktree(&wt_path).await.unwrap(), None);
        assert_eq!(
            git.inspect_worktree(plain.path().to_str().unwrap())
                .await
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn a_directory_inside_a_working_tree_is_not_a_worktree_of_its_own() {
        let repo = tempfile::tempdir().unwrap();
        init_repo_with_commit(repo.path()).await;
        let nested = repo.path().join("sub");
        std::fs::create_dir(&nested).unwrap();

        let inspection = Git::new()
            .inspect_worktree(nested.to_str().unwrap())
            .await
            .unwrap();

        assert_eq!(inspection, None);
    }
}
