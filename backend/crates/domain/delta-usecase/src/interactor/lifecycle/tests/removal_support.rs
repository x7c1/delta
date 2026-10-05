//! Shared setup for the session-removal tests: a closed session whose row says
//! where it ran and on which branch.

use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::ports::NewSession;

/// The repository every removal test's worktree is cut from — outside
/// [`TEST_WORKTREE_BASE`], as a real worktree spawn's `repo_root` is.
pub(super) const REPO_ROOT: &str = "/repos/delta";

/// The path a worktree spawn of `id` lands at under the test worktree base.
pub(super) fn worktree_path(id: &str) -> String {
    format!("{TEST_WORKTREE_BASE}/x7c1-delta-{id}")
}

/// Register `id` as a closed session that ran in `cwd`, launched against
/// `repo_root` on `branch` — the columns a spawn records and removal reads.
pub(super) async fn closed_session(
    ix: &TestInteractor,
    id: &str,
    cwd: &str,
    repo_root: Option<&str>,
    branch: Option<&str>,
) -> SessionId {
    let (session, _) = ix
        .store()
        .register_session(NewSession {
            id: id.into(),
            cwd: cwd.into(),
            transcript_path: format!("/transcripts/{id}.jsonl"),
            branch_at_launch: branch.map(str::to_owned),
            repo_root: repo_root.map(str::to_owned),
            repository_display_name: None,
        })
        .await
        .unwrap();
    assert!(ix.bound_pane(&session.id).await.is_none(), "starts closed");
    session.id
}
