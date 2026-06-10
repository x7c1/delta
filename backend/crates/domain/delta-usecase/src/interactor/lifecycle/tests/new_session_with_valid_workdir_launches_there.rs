use crate::interactor::testing::*;
use crate::SendTarget;

/// A composer-first send carrying a validated user-selected workdir launches
/// the fresh session in that directory's *canonical* path, not the default
/// `<base>/<token>`.
#[tokio::test]
async fn new_session_with_valid_workdir_launches_there() {
    let ix = interactor();
    // Mark the chosen directory as existing so validation succeeds.
    ix.workspace_fake()
        .existing_dirs
        .lock()
        .unwrap()
        .push("/projects/app".to_owned());

    ix.enqueue_send(
        SendTarget::NewSession {
            workdir: Some("/projects/app".to_owned()),
        },
        "hello",
        None,
    )
    .await
    .unwrap();

    let created = ix.tmux_fake().created.lock().unwrap().clone();
    assert_eq!(created.len(), 1, "one session spawned");
    assert_eq!(
        created[0].workdir,
        FakeWorkspace::canonical("/projects/app"),
        "the launch dir is the canonical user-selected path, not <base>/<token>"
    );
}
