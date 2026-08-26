use delta_model::SessionStatus;

use crate::interactor::testing::*;
use crate::ports::WorktreeStartPoint;
use crate::{SendTarget, WorktreeSpec};

/// A new-session send is answered *before* its worktree exists.
///
/// The build this defers is a `git fetch` plus a full checkout, which on a
/// large repository is seconds to tens of seconds — a wait the browser used to
/// spend unable to switch to the session it had just created, because the
/// launch sat inside the request. With the fake's worktree build held open, the
/// send still returns real ids and the row is already listed as `spawning`,
/// while nothing has been built or launched yet. Releasing the hold completes
/// the launch: the worktree lands on exactly the path the accept phase planned
/// (the one the session row already records as its `cwd`), the agent starts
/// there, and the spawn becomes pending — so the first hook binds it exactly as
/// it always has.
#[tokio::test]
async fn new_session_replies_before_the_worktree_is_built() {
    let gate = WorktreeGate::closed();
    let canonical = FakeWorkspace::canonical("/projects/app");
    let repo_root = "/projects/app/.git/..";
    let git = FakeGitWorktree::default()
        .with_repo(&canonical, repo_root)
        .with_origin_url(repo_root, "https://github.com/x7c1/delta.git")
        .with_gate(&gate);
    let ix = interactor_with_git(git);
    ix.workspace_fake()
        .existing_dirs
        .lock()
        .unwrap()
        .push("/projects/app".to_owned());

    let (send, _) = ix
        .enqueue_send(
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
        .expect("the send is accepted while the worktree build is still held");
    let session_id = send.session_id.clone();

    // The response carries real ids and the row is listed as `spawning`, so the
    // browser can switch to the session and watch it start.
    assert_ne!(send.id, 0, "the send row is persisted before the launch");
    let session = ix
        .store()
        .session(&session_id)
        .await
        .unwrap()
        .expect("the eager session row exists");
    assert_eq!(session.status, SessionStatus::Spawning);

    // …and none of the expensive work has run: the build is parked on the gate,
    // so nothing is checked out, trusted, or launched.
    assert!(
        ix.git_worktree_fake().created.lock().unwrap().is_empty(),
        "the worktree build has not run yet"
    );
    assert!(
        ix.tmux_fake().created.lock().unwrap().is_empty(),
        "no agent has been launched yet"
    );
    assert_eq!(
        ix.launching_session_ids().await,
        vec![session_id.clone()],
        "the session is accepted with its launch still in flight"
    );

    // Release the build and let the launch finish.
    gate.open();
    ix.await_launch().await;

    // The worktree landed on the planned path — the one the row already
    // records — on the per-session `delta-<id>` branch.
    let expected_path = format!("{TEST_WORKTREE_BASE}/x7c1-delta-{}", session_id.as_str());
    assert_eq!(
        session.cwd, expected_path,
        "the row recorded the planned launch dir before the build ran"
    );
    let created_worktrees = ix.git_worktree_fake().created.lock().unwrap().clone();
    assert_eq!(created_worktrees.len(), 1, "one worktree created");
    assert_eq!(created_worktrees[0].worktree_path, expected_path);
    assert_eq!(
        created_worktrees[0].branch,
        format!("delta-{}", session_id.as_str())
    );

    // The agent launched in it, and the spawn is now pending its first hook.
    let launched = ix.tmux_fake().created.lock().unwrap().clone();
    assert_eq!(launched.len(), 1, "one session spawned");
    assert_eq!(launched[0].workdir, expected_path);
    assert_eq!(ix.pending_session_ids().await, vec![session_id.clone()]);

    // Binding is unchanged: the launch's `SessionStart` claims the spawn.
    ix.on_session_start(session_start(session_id.as_str(), "startup"))
        .await
        .unwrap();
    assert!(
        ix.pane_for_session(&session_id).await.is_some(),
        "the first hook binds the launched spawn as it always has"
    );
}
