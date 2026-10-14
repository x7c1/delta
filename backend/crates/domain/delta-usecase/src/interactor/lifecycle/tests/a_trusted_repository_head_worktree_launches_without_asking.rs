use crate::ports::WorktreeStartPoint;

use super::launch_trust_support::*;

/// A new branch off the user's own `HEAD` in a repository Claude Code already
/// trusts carries exactly the content the user trusted: no confirmation is
/// asked, and the worktree is pre-accepted at launch.
#[tokio::test]
async fn a_trusted_repository_head_worktree_launches_without_asking() {
    let (ix, _sink) = interactor_over(repository());

    let check = check(&ix, &WorktreeStartPoint::Head).await;
    assert!(!check.needs_confirmation(), "nothing to ask, got {check:?}");
    assert!(check.agent_config_changes.is_empty());

    let (_, path) = launch(&ix, WorktreeStartPoint::Head, None).await;

    assert_eq!(
        pre_accepted(&ix),
        vec![path],
        "the worktree is pre-accepted"
    );
    assert!(
        recorded_repositories(&ix).is_empty(),
        "an already-trusted repository is not written again"
    );
    assert_eq!(ix.tmux_fake().created.lock().unwrap().len(), 1);
}
