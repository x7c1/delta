//! Shared setup for the Storage worktree tests.

use crate::interactor::testing::*;
use crate::ports::NewSession;

/// The repository every scripted worktree belongs to.
pub(super) const REPO_ROOT: &str = "/repos/delta";

/// The path of `name` under the test worktree base.
pub(super) fn under_base(name: &str) -> String {
    format!("{TEST_WORKTREE_BASE}/{name}")
}

/// Register a session that works in `cwd`.
pub(super) async fn session_in(ix: &TestInteractor, id: &str, cwd: &str) {
    session_on(ix, id, cwd, None).await;
}

/// Register a session that works in `cwd`, launched on `branch`.
pub(super) async fn session_on(ix: &TestInteractor, id: &str, cwd: &str, branch: Option<&str>) {
    ix.store()
        .register_session(NewSession {
            id: id.into(),
            cwd: cwd.into(),
            transcript_path: format!("/transcripts/{id}.jsonl"),
            branch_at_launch: branch.map(str::to_owned),
            repo_root: Some(REPO_ROOT.into()),
            repository_display_name: None,
        })
        .await
        .unwrap();
}
