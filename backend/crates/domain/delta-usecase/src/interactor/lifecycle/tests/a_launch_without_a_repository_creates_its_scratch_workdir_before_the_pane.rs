use crate::interactor::testing::*;
use crate::SendTarget;

/// A session launched with neither a repository nor a chosen directory gets its
/// per-token scratch directory created through the workspace before its pane.
///
/// Nothing else creates `<base>/<token>`, and tmux silently ignores a `-c`
/// directory that does not exist — the agent would start in the tmux server's
/// own working directory instead.
#[tokio::test]
async fn a_launch_without_a_repository_creates_its_scratch_workdir_before_the_pane() {
    let ix = interactor();

    ix.enqueue_send(
        SendTarget::NewSession {
            pull_request_number: None,
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

    let created = ix.tmux_fake().created.lock().unwrap().clone();
    assert_eq!(created.len(), 1);
    assert_eq!(
        *ix.workspace_fake().created_dirs.lock().unwrap(),
        vec![created[0].workdir.clone()],
        "the pane's directory is the one the workspace created"
    );
}
