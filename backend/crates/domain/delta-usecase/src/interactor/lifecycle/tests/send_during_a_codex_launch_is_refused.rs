use delta_model::{AgentProvider, SendStatus};

use crate::error::Error;
use crate::interactor::testing::*;
use crate::SendTarget;

/// A send arriving while a **Codex** session's launch is still running is
/// refused as `session_spawning`, not as an opaque agent error.
///
/// The Claude twin of this (`send_during_the_launch_window_is_refused`) covers
/// the pane path. The adapter-backed path needs its own because it reaches the
/// refusal differently: the enqueue's first move is to reconnect a *closed*
/// adapter-backed session, and a session whose launch has not finished has no
/// `provider_session_id` to reconnect with — so a gate placed after that block
/// would answer a `5xx` about a missing provider id instead of the honest
/// "still starting". The gate therefore sits ahead of both providers' paths,
/// and this pins it there.
#[tokio::test]
async fn send_during_a_codex_launch_is_refused() {
    let gate = ConnectGate::closed();
    let factory = FakeAgentFactory::gated("thr_gated", Some("turn_gated"), &gate);
    let ix = interactor_with_codex_factory(factory.clone());

    let (first, _) = ix
        .enqueue_send(
            SendTarget::NewSession {
                provider: AgentProvider::Codex,
                workdir: None,
                launch_option_ids: Vec::new(),
                worktree: None,
            },
            "first message",
            None,
        )
        .await
        .expect("the send is accepted while the connect is still held");
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
        "the refusal propagates as SessionSpawning, not as an agent error: {err:?}"
    );

    // Nothing reached the adapter, and the refused send left no row: the
    // accepted first prompt is still the only open send, still queued.
    assert!(
        factory.log().lock().unwrap().sends.is_empty(),
        "no turn was started by the refused send"
    );
    let open = ix.store().open_sends(&session_id).await.unwrap();
    assert_eq!(
        open.iter()
            .map(|send| (send.text.as_str(), send.status))
            .collect::<Vec<_>>(),
        vec![("first message", SendStatus::Queued)],
        "the refused send left no row behind"
    );

    // The held launch still completes normally once released, and the first
    // prompt is delivered then.
    gate.open();
    ix.await_launch().await;
    assert!(
        ix.is_session_open(&session_id).await,
        "the released launch bound its agent"
    );
    assert_eq!(
        factory.log().lock().unwrap().sends.clone(),
        vec!["first message".to_owned()],
        "the accepted first prompt is dispatched once the thread exists"
    );
}
