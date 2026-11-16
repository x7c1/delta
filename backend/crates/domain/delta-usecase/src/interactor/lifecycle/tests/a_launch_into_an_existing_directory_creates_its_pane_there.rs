use crate::interactor::testing::*;
use crate::SendTarget;

/// The launch-directory check does not block the happy path: a launch whose
/// directory exists creates its pane in that directory.
#[tokio::test]
async fn a_launch_into_an_existing_directory_creates_its_pane_there() {
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

    let created = ix.tmux_fake().created.lock().unwrap().clone();
    assert_eq!(created.len(), 1, "one pane is created");
    assert_eq!(created[0].workdir, FakeWorkspace::canonical(TEST_WORKDIR));
}
