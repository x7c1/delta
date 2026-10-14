use crate::interactor::testing::*;
use crate::ports::WorktreeStartPoint;

use super::launch_trust_support::*;

/// The check the new-session flow runs before sending creates nothing, so a
/// user who cancels the confirmation is left with no worktree, no session, no
/// pane and no change to Claude Code's config — cancelling sends nothing.
#[tokio::test]
async fn checking_launch_trust_creates_nothing() {
    let git = repository()
        .with_untrusted_repo(REPO_ROOT)
        .with_branch_commit("pr-head", "pr0003", &[(".mcp.json", "@@\n+{}\n")]);
    let (ix, _sink) = interactor_over(git);
    let start = WorktreeStartPoint::UseRemoteBranch("pr-head".to_owned());

    let check = check(&ix, &start).await;
    assert!(check.needs_confirmation());
    assert!(check.repository_untrusted);

    let git = ix.git_worktree_fake();
    assert!(
        git.created.lock().unwrap().is_empty(),
        "no worktree is created"
    );
    assert!(
        git.checked_out.lock().unwrap().is_empty(),
        "no worktree is checked out"
    );
    assert!(pre_accepted(&ix).is_empty(), "nothing is pre-accepted");
    assert!(
        recorded_repositories(&ix).is_empty(),
        "no trust is recorded"
    );
    assert!(
        ix.tmux_fake().created.lock().unwrap().is_empty(),
        "no pane is created"
    );
    assert!(
        ix.store()
            .list_sessions_page(None, 100)
            .await
            .unwrap()
            .is_empty(),
        "no session row is written"
    );
    assert!(ix.launching_session_ids().await.is_empty());
    assert_eq!(
        git.launch_commit_calls.lock().unwrap().clone(),
        vec![start],
        "the check resolved (and fetched) the branch the launch would build"
    );
}
