use crate::interactor::testing::*;
use crate::ports::{SessionLifecycle, WorktreeStartPoint};
use crate::{SendTarget, WorktreeSpec};

/// A cold start finds an accepted-but-not-yet-launched session live.
///
/// `POST /api/sessions` is idempotent over the single session, and the window
/// it has to cover now includes the launch preparation: between acceptance and
/// the agent starting there is no pane, no bound handle, and no pending spawn —
/// only the launching entry. If that did not read as live, a second cold start
/// arriving while the first session's worktree was being checked out would
/// start a rival session against the same working directory.
#[tokio::test]
async fn ensure_session_is_idempotent_while_a_launch_is_in_flight() {
    let gate = WorktreeGate::closed();
    let canonical = FakeWorkspace::canonical("/projects/app");
    let repo_root = "/projects/app/.git/..";
    let git = FakeGitWorktree::default()
        .with_repo(&canonical, repo_root)
        .with_gate(&gate);
    let ix = interactor_with_git(git);
    ix.workspace_fake()
        .existing_dirs
        .lock()
        .unwrap()
        .push("/projects/app".to_owned());

    // A composer-first send accepts a session whose worktree build is held.
    ix.enqueue_send(
        SendTarget::NewSession {
            provider: crate::AgentProvider::Claude,
            workdir: Some("/projects/app".to_owned()),
            launch_option_ids: Vec::new(),
            worktree: Some(WorktreeSpec {
                start_point: WorktreeStartPoint::Head,
            }),
        },
        "hello",
        None,
    )
    .await
    .unwrap();

    // The cold start finds it live and spawns nothing.
    let status = ix.ensure_session().await.unwrap();
    assert_eq!(status, SessionLifecycle::Ready);
    assert_eq!(
        ix.store().inner.lock().unwrap().sessions.len(),
        1,
        "a launching session must not be joined by a rival one"
    );

    gate.open();
    ix.await_launch().await;
    assert_eq!(
        ix.tmux_fake().created.lock().unwrap().len(),
        1,
        "exactly one agent was launched"
    );
}
