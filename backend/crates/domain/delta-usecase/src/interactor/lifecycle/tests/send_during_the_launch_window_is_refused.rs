use crate::interactor::testing::*;
use crate::ports::WorktreeStartPoint;
use crate::{SendTarget, WorktreeSpec};

/// A send arriving between acceptance and launch is refused as `session_spawning`.
///
/// The session is listed — and so reachable from the browser — from the moment
/// its first send is accepted, which is now *before* its agent has been
/// launched at all. A second send in that window must be refused for the same
/// reason it is refused against an already-launched-but-unbound spawn: no pane
/// is mapped and the transcript does not exist, so the ordinary closed-session
/// path (`ensure_open` → `claude --resume <id>`) would launch a rival agent
/// against a conversation nothing has written. The window this covers is the
/// wider one — the whole worktree build — which is exactly where a user is most
/// likely to type again.
#[tokio::test]
async fn send_during_the_launch_window_is_refused() {
    use crate::error::Error;

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

    let (first, _) = ix
        .enqueue_send(
            SendTarget::NewSession {
                provider: crate::AgentProvider::Claude,
                workdir: Some("/projects/app".to_owned()),
                launch_option_ids: Vec::new(),
                worktree: Some(WorktreeSpec {
                    start_point: WorktreeStartPoint::Head,
                }),
            },
            "first message",
            None,
        )
        .await
        .unwrap();
    let session_id = first.session_id.clone();
    assert_eq!(
        ix.launching_session_ids().await,
        vec![session_id.clone()],
        "the launch is still in flight"
    );

    let main = ix.store().main_thread_id(&session_id).await.unwrap();
    let err = ix
        .enqueue_send(to(main), "too early", None)
        .await
        .expect_err("a send to a session whose launch is still running must fail");
    assert!(
        matches!(err, Error::SessionSpawning(ref s) if s == session_id.as_str()),
        "the refusal propagates as SessionSpawning, got: {err:?}"
    );

    // Nothing was launched or typed, and the refused send left no row: the
    // accepted first prompt is still the only open send.
    assert!(
        ix.tmux_fake().created.lock().unwrap().is_empty(),
        "no agent was launched by the refused send"
    );
    assert!(
        ix.tmux_fake().sent.lock().unwrap().is_empty(),
        "no keystrokes were dispatched"
    );
    let open = ix.store().open_sends(&session_id).await.unwrap();
    assert_eq!(
        open.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(),
        vec!["first message"],
        "the refused send left no row behind"
    );

    // The held launch still completes normally once released.
    gate.open();
    ix.await_launch().await;
    assert_eq!(ix.pending_session_ids().await, vec![session_id]);
}
