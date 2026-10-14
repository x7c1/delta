use crate::interactor::testing::*;
use crate::ports::WorktreeStartPoint;

use super::launch_trust_support::*;

/// A repository Claude Code has never trusted is asked about, even for a
/// worktree off the user's own `HEAD`.
#[tokio::test]
async fn an_untrusted_repository_asks_before_its_head_worktree_launches() {
    let (ix, mut sink) = interactor_over(repository().with_untrusted_repo(REPO_ROOT));

    let check = check(&ix, &WorktreeStartPoint::Head).await;
    assert!(check.needs_confirmation());
    assert!(check.repository_untrusted);
    assert_eq!(check.commit.as_deref(), Some(FAKE_HEAD_COMMIT));
    assert!(check.agent_config_changes.is_empty());

    // A launch that skips the confirmation is not pre-accepted.
    launch(&ix, WorktreeStartPoint::Head, None).await;
    assert_stopped_for_confirmation(&ix, &mut sink);
}

/// Confirming an untrusted repository records trust for the repository root
/// itself, so the next launch from it does not ask again.
#[tokio::test]
async fn confirming_an_untrusted_repository_records_its_trust() {
    let (ix, _sink) = interactor_over(repository().with_untrusted_repo(REPO_ROOT));
    let commit = check(&ix, &WorktreeStartPoint::Head)
        .await
        .commit
        .expect("the check names the commit to confirm");

    let (_, path) = launch(&ix, WorktreeStartPoint::Head, Some(&commit)).await;

    assert_eq!(
        pre_accepted(&ix),
        vec![path],
        "the worktree is pre-accepted"
    );
    assert_eq!(
        recorded_repositories(&ix),
        vec![REPO_ROOT.to_owned()],
        "the repository root is recorded as trusted"
    );
    assert!(
        !check(&ix, &WorktreeStartPoint::Head)
            .await
            .needs_confirmation(),
        "the next HEAD launch from the same repository does not ask"
    );
}
