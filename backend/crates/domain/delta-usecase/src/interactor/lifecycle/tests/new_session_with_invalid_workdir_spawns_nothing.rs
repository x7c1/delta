use crate::interactor::testing::*;
use crate::SendTarget;

/// An invalid user-selected workdir is rejected before anything is spawned: the
/// send returns `InvalidWorkdir`, no tmux session is created, no settings are
/// written, and no pending spawn is left behind to bind later.
#[tokio::test]
async fn new_session_with_invalid_workdir_spawns_nothing() {
    let ix = interactor();
    // `/nope` is not in `existing_dirs`, so validation fails.

    let err = ix
        .enqueue_send(
            SendTarget::NewSession {
                pull_request_number: None,
                provider: crate::AgentProvider::Claude,
                workdir: Some("/nope".to_owned()),
                launch_option_ids: Vec::new(),
                worktree: None,
            },
            "hello",
            None,
        )
        .await
        .unwrap_err();

    assert!(
        matches!(err, crate::error::Error::InvalidWorkdir(_)),
        "an invalid workdir is rejected as InvalidWorkdir, got {err:?}"
    );
    assert!(
        ix.tmux_fake().created.lock().unwrap().is_empty(),
        "no pane is created for an invalid workdir"
    );
    assert!(
        ix.workspace_fake().written.lock().unwrap().is_empty(),
        "settings are not written when validation fails first"
    );
    assert!(
        ix.pending_session_ids().await.is_empty(),
        "no pending spawn is recorded for an invalid workdir"
    );
}
