use crate::interactor::testing::*;
use crate::SendTarget;

/// The scratch directory is created *before* the pane: when it cannot be, the
/// launch fails and no agent is started anywhere else.
#[tokio::test]
async fn a_scratch_workdir_that_cannot_be_created_launches_no_pane() {
    let ix = interactor();
    *ix.workspace_fake().fail_create_dir.lock().unwrap() = true;

    let (send, _) = ix
        .enqueue_send(
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

    assert!(
        ix.tmux_fake().created.lock().unwrap().is_empty(),
        "no pane is created in a directory that does not exist"
    );
    assert_eq!(
        ix.store()
            .session(&send.session_id)
            .await
            .unwrap()
            .expect("the row of a failed launch is kept")
            .status,
        delta_model::SessionStatus::Failed,
    );
}
