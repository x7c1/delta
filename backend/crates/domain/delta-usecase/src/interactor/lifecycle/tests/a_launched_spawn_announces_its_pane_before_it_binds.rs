use crate::interactor::testing::*;
use crate::ports::SessionEvent;
use crate::SendTarget;

/// A launch that comes up announces its pane, on the seam the browser listens on.
///
/// Between the accept and the bind the session row reads `spawning` throughout,
/// so nothing in it distinguishes "still checking out a worktree" (nothing to
/// attach to) from "the agent is running in a pane nobody can see". This event
/// is that distinction, and it is what lets the browser attach its terminal to a
/// session that has not bound — the only way to answer a prompt the launch has
/// stopped on.
///
/// It rides the async seam, like the failure report from the same window: the
/// REST caller was answered long before the background launch reported in.
#[tokio::test]
async fn a_launched_spawn_announces_its_pane_before_it_binds() {
    let (ix, mut sink) = interactor_with_event_sink();

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
        .expect("the send is accepted before the launch runs");
    let session_id = send.session_id.clone();

    ix.await_launch().await;

    assert_eq!(
        sink.try_recv().expect("the launch announced its pane"),
        SessionEvent::SpawnPaneReady {
            session_id: session_id.clone(),
            pane_token: "delta-1".to_owned(),
        },
        "the event names the session and the tmux session behind it"
    );
    assert!(
        !ix.is_session_open(&session_id).await,
        "announced while still starting: the bind has not happened"
    );
}
