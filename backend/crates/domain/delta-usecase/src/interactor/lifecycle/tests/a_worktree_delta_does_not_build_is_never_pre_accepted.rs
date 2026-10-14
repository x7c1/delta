use crate::interactor::testing::*;
use crate::ports::WorktreeStartPoint;

use super::launch_trust_support::*;

/// A `use_remote_branch` start whose branch is already checked out outside
/// Delta's worktree base reuses that directory, which Delta never
/// pre-accepts: the check has nothing to ask, and the launch leaves the
/// directory to Claude Code's own trust dialog.
#[tokio::test]
async fn a_reused_directory_outside_the_worktree_base_is_never_pre_accepted() {
    let outside = "/projects/app-other-checkout";
    let git = repository()
        .with_branch_checked_out("feature", outside)
        .with_branch_commit(
            "feature",
            "feat0002",
            &[(".claude/settings.json", "@@\n+x\n")],
        );
    let (ix, _sink) = interactor_over(git);
    let start = WorktreeStartPoint::UseRemoteBranch("feature".to_owned());

    let check = check(&ix, &start).await;
    assert!(
        !check.needs_confirmation(),
        "nothing Delta pre-accepts to ask about"
    );
    assert_eq!(check.commit, None);

    launch(&ix, start, None).await;

    assert!(
        pre_accepted(&ix).is_empty(),
        "the outside directory is not pre-accepted"
    );
    let created = ix.tmux_fake().created.lock().unwrap().clone();
    assert_eq!(created.len(), 1, "the session still launches there");
    assert_eq!(created[0].workdir, outside);
}

/// A provider that does not run Claude Code in a pane has no Claude Code
/// trust to pre-accept, so its worktree launches are never asked about.
#[tokio::test]
async fn a_provider_without_a_claude_pane_is_never_asked() {
    let git = repository().with_untrusted_repo(REPO_ROOT);
    let (ix, _sink) = interactor_over(git);
    let ix = ix.with_adapter_factory(FakeAgentFactory::new("thr_1", None));

    let check = ix
        .check_launch_trust(
            WORKDIR,
            &WorktreeStartPoint::Head,
            crate::AgentProvider::Codex,
        )
        .await
        .unwrap();

    assert!(!check.needs_confirmation());
    assert!(ix
        .git_worktree_fake()
        .launch_commit_calls
        .lock()
        .unwrap()
        .is_empty());
}
