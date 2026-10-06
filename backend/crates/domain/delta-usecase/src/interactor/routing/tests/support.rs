//! Shared setup for the erase tests.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::ports::NewSession;

/// The repository every scripted worktree is cut from — outside
/// [`TEST_WORKTREE_BASE`], as a real worktree spawn's `repo_root` is.
pub(super) const REPO_ROOT: &str = "/repos/delta";

/// The directory holding the worktree base, as the transport names `~/.delta`
/// when the base is the default.
pub(super) const BASE_PARENT: &str = "/home/u/.delta";

/// A fixed "now" past every fixture's timestamp.
pub(super) fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(4_102_444_800) // 2100-01-01
}

/// The path of `name` under the test worktree base.
pub(super) fn under_base(name: &str) -> String {
    format!("{TEST_WORKTREE_BASE}/{name}")
}

/// Register `id` as a closed session that ran in the worktree Delta cut for
/// it, `<base>/x7c1-delta-<id>` on `delta-<id>`.
pub(super) async fn closed_worktree_session(ix: &TestInteractor, id: &str) -> SessionId {
    let (session, _) = ix
        .store()
        .register_session(NewSession {
            id: id.into(),
            cwd: worktree_of(id),
            transcript_path: format!("/transcripts/{id}.jsonl"),
            branch_at_launch: Some(format!("delta-{id}")),
            repo_root: Some(REPO_ROOT.into()),
            repository_display_name: None,
        })
        .await
        .unwrap();
    session.id
}

/// The worktree [`closed_worktree_session`] records for `id`.
pub(super) fn worktree_of(id: &str) -> String {
    under_base(&format!("x7c1-delta-{id}"))
}
