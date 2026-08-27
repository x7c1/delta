use std::time::Duration;

use delta_model::{AgentProvider, MessageUuid, SendStatus};

use crate::agent::{AgentEvent, TurnStatus};
use crate::error::Error;
use crate::interactor::testing::*;
use crate::SendTarget;

/// Poll `f` until it returns `true`, or panic after a short deadline. The event
/// pump runs on a background task, so a turn end — and the queue flush it
/// triggers — lands asynchronously; this yields to it between checks.
async fn wait_until(what: &str, mut f: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !f() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// A send arriving while a **Codex** session's launch is still running is
/// queued and delivered afterwards — the adapter-backed twin of
/// `send_to_a_still_spawning_session_is_queued`.
///
/// The adapter-backed path needs its own coverage because it reaches the
/// spawning gate differently: the enqueue's first move is to reconnect a
/// *closed* adapter-backed session, and a session whose launch has not finished
/// has no `provider_session_id` to reconnect with — so a gate placed after that
/// block would answer a `5xx` about a missing provider id instead of accepting
/// the send. The gate therefore sits ahead of both providers' paths.
///
/// And the flush is different too: a Codex session has no `Stop` hook, so the
/// queue is drained by the turn end its event pump reports. Without that this
/// row would sit `queued` forever.
#[tokio::test]
async fn send_during_a_codex_launch_is_queued() {
    let gate = ConnectGate::closed();
    let factory = FakeAgentFactory::gated("thr_gated", Some("turn_gated"), &gate);
    let turn_events = factory.event_sender();
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
    let (queued, events) = ix
        .enqueue_send(to(main), "and one more while it starts", None)
        .await
        .expect("a plain send to a still-starting Codex session is accepted");
    assert_eq!(queued.status, SendStatus::Queued);
    assert!(
        events.is_empty(),
        "accepting a queued send announces nothing, got {events:?}"
    );

    // A branch send is still refused: the session has ingested no message to
    // branch from, whatever the provider.
    let err = ix
        .enqueue_send(
            branch_off(main, &MessageUuid::from("no-such-message")),
            "branch from nothing",
            Some("a quote"),
        )
        .await
        .expect_err("a branch send to a still-starting session must fail");
    assert!(
        matches!(err, Error::SessionSpawning(ref id) if id == session_id.as_str()),
        "the refusal propagates as SessionSpawning, not as an agent error: {err:?}"
    );

    // Nothing reached the adapter yet: the accepted row did not touch the
    // launch, and the refused branch left no row at all.
    assert!(
        factory.log().lock().unwrap().sends.is_empty(),
        "no turn was started while the launch was still held"
    );
    assert_eq!(
        ix.store()
            .open_sends(&session_id)
            .await
            .unwrap()
            .iter()
            .map(|send| (send.text.clone(), send.status))
            .collect::<Vec<_>>(),
        vec![
            ("first message".to_owned(), SendStatus::Queued),
            (
                "and one more while it starts".to_owned(),
                SendStatus::Queued
            ),
        ],
    );

    // Release the launch: the bind promotes and dispatches the first prompt,
    // and only that one — the second send waits its turn. The queue flush the
    // bind runs afterwards (for the launches that start no turn at all) finds
    // this turn in flight, so it must not push the second send out early.
    gate.open();
    ix.await_launch().await;
    assert!(
        ix.is_session_open(&session_id).await,
        "the released launch bound its agent"
    );
    assert_eq!(
        factory.log().lock().unwrap().sends.clone(),
        vec!["first message".to_owned()],
        "the opening turn carries the first prompt alone, dispatched once"
    );

    // The provider ends that turn; the pump's turn end flushes the queue, so
    // the second message reaches `turn/start` right behind the first.
    turn_events
        .send(AgentEvent::TurnCompleted {
            status: TurnStatus::Completed,
        })
        .expect("the pump is draining the adapter");
    wait_until("the queued send to reach the adapter", || {
        factory.log().lock().unwrap().sends.len() == 2
    })
    .await;
    assert_eq!(
        factory.log().lock().unwrap().sends.clone(),
        vec![
            "first message".to_owned(),
            "and one more while it starts".to_owned(),
        ],
        "the queued send is delivered once, after the first prompt"
    );
    assert!(
        ix.store().open_sends(&session_id).await.unwrap().is_empty(),
        "both rows completed at their turn/start acknowledgement"
    );
}
