use crate::interactor::testing::*;
use crate::SendTarget;

/// A fresh session in a selected directory outside any git repository, with no
/// worktree, is not a trust candidate — so trust is NOT seeded (avoids bloating
/// the user config for ordinary sessions).
#[tokio::test]
async fn new_session_outside_a_repository_does_not_seed_trust() {
    let ix = interactor();

    ix.enqueue_send(
        SendTarget::NewSession {
            pull_request_number: None,
            provider: crate::AgentProvider::Claude,
            workdir: TEST_WORKDIR.to_owned(),
            launch_option_ids: Vec::new(),
            worktree: None,
        },
        "hello",
        None,
    )
    .await
    .unwrap();

    ix.await_launch().await;

    assert!(
        ix.git_worktree_fake().trusted.lock().unwrap().is_empty(),
        "a directory outside any repository is not seeded for trust"
    );
}
