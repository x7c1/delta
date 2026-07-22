use delta_model::{AgentProvider, MessageUuid, SendStatus, SessionStatus};

use crate::interactor::testing::*;
use crate::turn::TurnState;
use crate::SendTarget;

/// A composer-first send with `provider: Codex` creates a session over the
/// adapter (no tmux pane), persists the provider-minted ids and activates the
/// row, completes the first send at the `turn/start` acknowledgement, and reads
/// as open-without-pane.
#[tokio::test]
async fn new_session_with_codex_provider_creates_a_terminal_less_session() {
    let factory = FakeAgentFactory::new("thr_fake", Some("turn_fake"));
    let ix = interactor_with_codex_factory(factory.clone());

    let (send, events) = ix
        .enqueue_send(
            SendTarget::NewSession {
                provider: AgentProvider::Codex,
                workdir: None,
                launch_option_ids: Vec::new(),
                worktree: None,
            },
            "hello codex",
            None,
        )
        .await
        .unwrap();
    let session_id = send.session_id.clone();
    assert!(
        events.is_empty(),
        "no synchronous events from a codex create"
    );

    // Terminal-less: no tmux session was ever created.
    assert!(
        ix.tmux_fake().created.lock().unwrap().is_empty(),
        "a Codex session must not spawn a tmux pane"
    );

    // The session row is persisted as Codex, activated (spawning → active by
    // set_provider_ids), and carries the provider-minted ids (session ↔ thread
    // is 1:1, so both equal the thread id).
    let session = ix.store().session(&session_id).await.unwrap().unwrap();
    assert_eq!(session.provider, AgentProvider::Codex);
    assert_eq!(
        session.status,
        SessionStatus::Active,
        "recording the provider ids activates the spawning row"
    );
    assert_eq!(session.provider_session_id.as_deref(), Some("thr_fake"));
    assert_eq!(session.provider_thread_id.as_deref(), Some("thr_fake"));

    // The first send is completed at the turn/start ack: marked matched to the
    // provider turn id, not left dispatched, and no longer in the open set.
    let stored = ix.store().send(send.id).await.unwrap().unwrap();
    assert_eq!(
        stored.status,
        SendStatus::Matched,
        "the Codex send is completed by the turn/start ack, not by an echo"
    );
    assert_eq!(stored.matched_uuid, Some(MessageUuid::from("turn_fake")));
    assert!(
        ix.store().open_sends(&session_id).await.unwrap().is_empty(),
        "a completed Codex send does not linger in the open list"
    );

    // Open-without-pane: the session reads as open, but there is nothing for the
    // PTY bridge to attach to.
    assert!(
        ix.is_session_open(&session_id).await,
        "a live Codex session is open"
    );
    assert_eq!(
        ix.pane_for_session(&session_id).await,
        None,
        "a terminal-less session exposes no pane"
    );

    // The adapter was actually driven: one launch (thread/start) and one send
    // (turn/start) carrying the visible prompt. Snapshot the log and drop the
    // guard before the awaits below.
    let (launch_count, launch_first_prompt, sends) = {
        let log = factory.log();
        let log = log.lock().unwrap();
        (
            log.launches.len(),
            log.launches[0].first_prompt.clone(),
            log.sends.clone(),
        )
    };
    assert_eq!(launch_count, 1, "one launch (thread/start)");
    assert_eq!(
        launch_first_prompt, None,
        "the first prompt is delivered as its own turn, not on launch, so the ack completes the send"
    );
    assert_eq!(
        sends,
        vec!["hello codex".to_owned()],
        "the visible prompt reached the adapter's send"
    );

    // The turn is tracked ExternalPrompt-style: InFlight with no send id, so the
    // FSM never references the completed send.
    let live = ix.live_state_for(&session_id).await;
    assert_eq!(live.turn, TurnState::InFlight { send_id: None });
}
