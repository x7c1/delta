use crate::ports::WorktreeStartPoint;

use super::launch_trust_support::*;

/// The confirmation is bound to the commit the user was shown: when the
/// worktree the launch builds is on a different commit (the branch moved
/// between the check and the build), trust is not pre-accepted and the launch
/// stops, so the user is asked again about what is actually there.
#[tokio::test]
async fn a_confirmation_for_another_commit_is_not_honoured() {
    let git = repository().with_branch_commit(
        "pr-head",
        "shown0001",
        &[(".claude/settings.json", "@@\n-a\n+b\n")],
    );
    let (ix, mut sink) = interactor_over(git);
    let start = WorktreeStartPoint::UseRemoteBranch("pr-head".to_owned());
    let shown = check(&ix, &start).await.commit.unwrap();
    // The branch moves before the build checks it out.
    ix.git_worktree_fake().branches.lock().unwrap()[0].commit = "moved0002".to_owned();

    launch(&ix, start, Some(&shown)).await;

    assert_stopped_for_confirmation(&ix, &mut sink);
}

/// The same holds for an untrusted repository: a confirmation naming a
/// commit other than the one the built worktree is on (the user's `HEAD`
/// moved after they were asked) is not honoured, and the repository is not
/// recorded as trusted on the strength of it.
#[tokio::test]
async fn a_confirmation_for_another_head_in_an_untrusted_repository_is_not_honoured() {
    let (ix, mut sink) = interactor_over(repository().with_untrusted_repo(REPO_ROOT));

    launch(&ix, WorktreeStartPoint::Head, Some("an-earlier-head")).await;

    assert_stopped_for_confirmation(&ix, &mut sink);
}
