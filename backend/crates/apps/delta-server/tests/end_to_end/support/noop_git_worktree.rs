//! A git worktree gateway that never runs `git`.

use async_trait::async_trait;

use delta_usecase::{
    BranchDeletion, GitWorktree, RemoteBranches, WorktreeRemoval, WorktreeStartPoint,
};

/// A no-op `GitWorktree` so the end-to-end flow (which never requests a
/// worktree) wires without touching `git`.
pub(super) struct NoopGitWorktree;

#[async_trait]
impl GitWorktree for NoopGitWorktree {
    async fn repo_root(&self, _path: &str) -> delta_usecase::Result<Option<String>> {
        Ok(None)
    }

    async fn current_branch(&self, _path: &str) -> delta_usecase::Result<Option<String>> {
        Ok(None)
    }

    async fn default_branch(&self, _repo_root: &str) -> delta_usecase::Result<Option<String>> {
        Ok(None)
    }

    async fn origin_url(&self, _path: &str) -> delta_usecase::Result<Option<String>> {
        Ok(None)
    }

    async fn fetch_remote_branches(
        &self,
        _repo_root: &str,
    ) -> delta_usecase::Result<RemoteBranches> {
        Ok(RemoteBranches {
            default_branch: None,
            branches: Vec::new(),
        })
    }

    async fn create_worktree(
        &self,
        _repo_root: &str,
        _worktree_path: &str,
        _branch: &str,
        _start_point: WorktreeStartPoint,
    ) -> delta_usecase::Result<()> {
        Ok(())
    }

    async fn worktree_path_for_branch(
        &self,
        _repo_root: &str,
        _branch: &str,
    ) -> delta_usecase::Result<Option<String>> {
        Ok(None)
    }

    async fn add_worktree_checkout(
        &self,
        _repo_root: &str,
        _worktree_path: &str,
        _branch: &str,
    ) -> delta_usecase::Result<()> {
        Ok(())
    }

    async fn ensure_dir_trusted(&self, _dir: &str) -> delta_usecase::Result<()> {
        // The end-to-end flow launches in a non-git scratch dir (`repo_root`
        // returns `None`), so trust-seeding is never reached; a no-op suffices.
        Ok(())
    }

    // The end-to-end flow never runs in a Delta worktree, so removing a
    // session never reaches the worktree cleanup; these are never called.

    async fn remove_worktree(
        &self,
        _repo_root: &str,
        _path: &str,
    ) -> delta_usecase::Result<WorktreeRemoval> {
        Ok(WorktreeRemoval::KeptDirty)
    }

    async fn delete_branch_if_merged(
        &self,
        _repo_root: &str,
        _branch: &str,
    ) -> delta_usecase::Result<BranchDeletion> {
        Ok(BranchDeletion::KeptUnmerged)
    }

    async fn is_branch_merged(
        &self,
        _repo_root: &str,
        _branch: &str,
    ) -> delta_usecase::Result<Option<bool>> {
        Ok(Some(false))
    }

    async fn prune_worktrees(&self, _repo_root: &str) -> delta_usecase::Result<()> {
        Ok(())
    }

    async fn forget_dir_trusted(&self, _dir: &str) -> delta_usecase::Result<()> {
        Ok(())
    }

    async fn inspect_worktree(
        &self,
        _path: &str,
    ) -> delta_usecase::Result<Option<delta_usecase::WorktreeInspection>> {
        Ok(None)
    }

    async fn force_remove_worktree(
        &self,
        _repo_root: &str,
        _path: &str,
    ) -> delta_usecase::Result<()> {
        Ok(())
    }
}
