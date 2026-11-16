use delta_model::SessionId;

use crate::error::Error;
use crate::interactor::testing::*;

/// Resuming a closed session whose cwd no longer exists fails with
/// `LaunchDirMissing` naming that directory, instead of letting tmux start the
/// resumed pane in the home directory. No pane is created, no settings are
/// written, and the session stays closed.
#[tokio::test]
async fn open_session_with_a_missing_cwd_creates_no_pane() {
    let ix = interactor_with_workspace_and_git(
        FakeWorkspace::default().with_missing_dir("/elsewhere"),
        FakeGitWorktree::default(),
    );
    ix.on_user_prompt_submit(submit_in(
        "sess-R",
        "/elsewhere/t.jsonl",
        "/elsewhere",
        "seed",
    ))
    .await
    .unwrap();
    let id = SessionId::from("sess-R");
    assert!(ix.bound_pane(&id).await.is_none(), "starts closed");

    let err = ix
        .open_session(&id)
        .await
        .expect_err("a resume into a missing directory fails");
    assert!(
        matches!(err, Error::LaunchDirMissing(ref dir) if dir == "/elsewhere"),
        "the missing cwd is named, got: {err:?}"
    );
    assert!(
        err.to_string().contains("/elsewhere"),
        "the message names the missing directory: {err}"
    );

    assert!(
        ix.tmux_fake().created.lock().unwrap().is_empty(),
        "no pane is created in a directory that does not exist"
    );
    assert!(
        ix.workspace_fake().written.lock().unwrap().is_empty(),
        "no session settings are written for a refused resume"
    );
    assert!(
        ix.bound_pane(&id).await.is_none(),
        "the session stays closed"
    );
}
