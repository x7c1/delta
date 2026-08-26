use crate::interactor::testing::*;
use crate::SendTarget;

/// A fresh session with no user workdir and no worktree launches in the default
/// `<base>/<token>` scratch directory, which is empty and never triggers Claude
/// Code's trust dialog — so trust is NOT seeded (avoids bloating the user config
/// for ordinary sessions).
#[tokio::test]
async fn new_session_with_default_workdir_does_not_seed_trust() {
    let ix = interactor();

    ix.enqueue_send(
        SendTarget::NewSession {
            provider: crate::AgentProvider::Claude,
            workdir: None,
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
        "the default scratch dir is not seeded for trust"
    );
}
