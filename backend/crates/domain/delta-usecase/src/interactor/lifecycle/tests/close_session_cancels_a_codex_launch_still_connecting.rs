use delta_model::AgentProvider;

use crate::interactor::testing::*;
use crate::ports::SessionEvent;
use crate::SendTarget;

/// The adapter-backed twin of `close_session_cancels_a_launch_still_preparing`:
/// a Codex session closed while its `connect` is still held is cancelled, and
/// the connection the launch goes on to make is dropped rather than bound.
///
/// An adapter launch is *only ever* in this shape while it starts — it never
/// becomes a pending spawn, because its bind is the launch's own last step — so
/// without this path a terminal-less session whose provider hangs could not be
/// closed at all. The event carries no `pane_token`: this session never had a
/// pane, so tmux is kept out of the cancellation entirely.
#[tokio::test]
async fn close_session_cancels_a_codex_launch_still_connecting() {
    let gate = ConnectGate::closed();
    let factory = FakeAgentFactory::gated("thr_gated", Some("turn_gated"), &gate);
    let ix = interactor_with_codex_factory(factory.clone());

    let (send, _) = ix
        .enqueue_send(
            SendTarget::NewSession {
                pull_request_number: None,
                provider: AgentProvider::Codex,
                workdir: None,
                launch_option_ids: Vec::new(),
                worktree: None,
            },
            "start something",
            None,
        )
        .await
        .expect("the send is accepted while the connect is still held");
    let session_id = send.session_id.clone();
    assert_eq!(
        ix.launching_session_ids().await,
        vec![session_id.clone()],
        "the launch is parked on the connect gate"
    );

    let events = ix.close_session(&session_id).await.unwrap();

    let [SessionEvent::SpawnFailed {
        session_id: failed_id,
        pane_token,
        reason,
        cancelled,
        unsent,
    }] = events.as_slice()
    else {
        panic!("expected exactly one SpawnFailed, got {events:?}");
    };
    assert_eq!(
        failed_id, &session_id,
        "the failure names the closed session"
    );
    assert_eq!(
        *pane_token, None,
        "a terminal-less launch reports no pane token"
    );
    assert_eq!(reason.as_deref(), Some("closed while starting"));
    assert!(
        *cancelled,
        "the user asked for this, so the browser words it as a cancel"
    );
    assert_eq!(
        unsent.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(),
        vec!["start something"],
        "the queued first prompt is handed back to the composer"
    );
    assert!(
        ix.store().session(&session_id).await.unwrap().is_none(),
        "the eager Codex row of a cancelled launch is deleted"
    );
    assert!(
        ix.tmux_fake().created.lock().unwrap().is_empty()
            && ix.tmux_fake().killed.lock().unwrap().is_empty(),
        "a Codex spawn never touches tmux, cancelled or not"
    );

    // Release the held connect and let the launch finish: it stands the
    // provider up, finds nothing to bind it to, and drops the connection.
    gate.open();
    ix.await_launch().await;

    assert!(
        !ix.is_session_open(&session_id).await,
        "the abandoned checkpoint binds no agent"
    );
    assert!(
        ix.launching_session_ids().await.is_empty(),
        "the launching entry stays gone"
    );
    assert!(
        factory.log().lock().unwrap().sends.is_empty(),
        "the cancelled launch's first prompt was never started as a turn"
    );
    assert!(
        ix.store().session(&session_id).await.unwrap().is_none(),
        "the finished launch does not resurrect the deleted row"
    );
}
