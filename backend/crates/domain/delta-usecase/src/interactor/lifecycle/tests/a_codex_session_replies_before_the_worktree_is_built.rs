use delta_model::{AgentProvider, SendStatus, SessionStatus};

use crate::interactor::testing::*;
use crate::ports::{SessionEvent, WorktreeStartPoint};
use crate::{SendTarget, WorktreeSpec};

/// A new **Codex** session's send is answered *before* its worktree exists and
/// before its adapter has connected.
///
/// This is the Claude accept→launch split applied to the adapter-backed path
/// (compare `new_session_replies_before_the_worktree_is_built`). Starting a
/// Codex session from a PR used to hold the composer on the new-session screen
/// for the whole `git fetch` + checkout *and* the `codex app-server` handshake,
/// because all of it sat inside the request. With the worktree build held open,
/// the send now returns real ids and the row is already listed as `spawning`,
/// while nothing has been built and no adapter has been connected.
///
/// Releasing the hold completes the launch: the row activates with the
/// provider-minted ids, `session_registered` goes out (the browser's release
/// signal for the spawn it is tracking — a Codex spawn had no such signal at
/// all before), and the first prompt — accepted as a `queued` row, since nothing
/// had received it — is promoted and reaches the adapter's `turn/start`.
#[tokio::test]
async fn a_codex_session_replies_before_the_worktree_is_built() {
    let gate = WorktreeGate::closed();
    let canonical = FakeWorkspace::canonical("/projects/app");
    let repo_root = "/projects/app/.git/..";
    let git = FakeGitWorktree::default()
        .with_repo(&canonical, repo_root)
        .with_origin_url(repo_root, "https://github.com/x7c1/delta.git")
        .with_gate(&gate);
    let factory = FakeAgentFactory::new("thr_deferred", Some("turn_deferred"));
    let (ix, mut sink) = interactor_with_git_and_codex_factory_and_event_sink(git, factory.clone());
    ix.workspace_fake()
        .existing_dirs
        .lock()
        .unwrap()
        .push("/projects/app".to_owned());

    let (send, _) = ix
        .enqueue_send(
            SendTarget::NewSession {
                pull_request_number: None,
                provider: AgentProvider::Codex,
                workdir: Some("/projects/app".to_owned()),
                launch_option_ids: Vec::new(),
                worktree: Some(WorktreeSpec {
                    start_point: WorktreeStartPoint::Head,
                }),
            },
            "hello codex",
            None,
        )
        .await
        .expect("the send is accepted while the worktree build is still held");
    let session_id = send.session_id.clone();

    // The response carries real ids and the row is listed as `spawning`, so the
    // browser can switch to the session and watch it start.
    assert_ne!(send.id, 0, "the send row is persisted before the launch");
    let accepted = ix
        .store()
        .session(&session_id)
        .await
        .unwrap()
        .expect("the eager session row exists");
    assert_eq!(accepted.status, SessionStatus::Spawning);
    assert_eq!(
        accepted.provider_session_id, None,
        "no provider thread exists yet, so the ids are still NULL"
    );

    // The first prompt is `queued`, not `dispatched`: unlike Claude — where it
    // rides on the launch argv — nothing has received it yet.
    assert_eq!(
        ix.store().send(send.id).await.unwrap().unwrap().status,
        SendStatus::Queued,
        "the accepted first prompt waits for a provider thread to exist"
    );

    // …and none of the expensive work has run: the build is parked on the gate,
    // so no worktree is checked out and no adapter has been connected.
    assert!(
        ix.git_worktree_fake().created.lock().unwrap().is_empty(),
        "the worktree build has not run yet"
    );
    assert!(
        factory.log().lock().unwrap().launches.is_empty(),
        "no adapter has been connected yet"
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
    // recorded as its `cwd` — and the adapter was launched there.
    let expected_path = format!("{TEST_WORKTREE_BASE}/x7c1-delta-{}", session_id.as_str());
    assert_eq!(
        accepted.cwd, expected_path,
        "the row recorded the planned launch dir before the build ran"
    );
    let (launch_workdirs, sends) = {
        let log = factory.log();
        let log = log.lock().unwrap();
        (
            log.launches
                .iter()
                .map(|launch| launch.workdir.clone())
                .collect::<Vec<_>>(),
            log.sends.clone(),
        )
    };
    assert_eq!(
        launch_workdirs,
        vec![expected_path],
        "one thread/start, in the built worktree"
    );

    // The row is live, carrying the provider-minted ids.
    let session = ix.store().session(&session_id).await.unwrap().unwrap();
    assert_eq!(session.status, SessionStatus::Active);
    assert_eq!(session.provider_session_id.as_deref(), Some("thr_deferred"));
    assert_eq!(session.provider_thread_id.as_deref(), Some("thr_deferred"));

    // The first prompt was promoted and delivered as the opening turn, and
    // completed at the `turn/start` acknowledgement.
    assert_eq!(
        sends,
        vec!["hello codex".to_owned()],
        "the accepted first prompt reached the adapter's turn/start"
    );
    assert!(
        ix.store().open_sends(&session_id).await.unwrap().is_empty(),
        "the dispatched first send is completed by the ack, not left open"
    );

    // The browser is told the session came up: this is what releases its
    // tracked spawn and re-enables the composer.
    let mut events = Vec::new();
    while let Ok(event) = sink.try_recv() {
        events.push(event);
    }
    assert!(
        events.contains(&SessionEvent::SessionRegistered {
            session_id: session_id.clone(),
        }),
        "a bound Codex session announces itself, got {events:?}"
    );
    assert!(
        ix.launching_session_ids().await.is_empty(),
        "the launching entry is settled by the bind"
    );
}
