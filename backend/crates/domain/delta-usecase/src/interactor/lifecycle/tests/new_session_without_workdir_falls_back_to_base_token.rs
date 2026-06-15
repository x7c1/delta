use crate::interactor::testing::*;
use crate::SendTarget;

/// A composer-first send with no workdir keeps today's behaviour: the session
/// launches in the default per-token `<base>/<token>` directory.
#[tokio::test]
async fn new_session_without_workdir_falls_back_to_base_token() {
    let ix = interactor();

    ix.enqueue_send(
        SendTarget::NewSession {
            workdir: None,
            launch_option_ids: Vec::new(),
            worktree: None,
        },
        "hello",
        None,
    )
    .await
    .unwrap();

    let created = ix.tmux_fake().created.lock().unwrap().clone();
    assert_eq!(created.len(), 1);
    assert_eq!(created[0].workdir, "/work/delta-1", "<base>/<token>");
}
