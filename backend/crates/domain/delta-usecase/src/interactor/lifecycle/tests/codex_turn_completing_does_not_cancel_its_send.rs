use delta_model::{AgentProvider, SendStatus};

use crate::interactor::testing::*;
use crate::turn::{TurnInput, TurnState};
use crate::SendTarget;

/// The turn-start / send-row FSM decision, at the interactor level: a Codex turn
/// is tracked as consuming no send (`InFlight { send_id: None }`), so when the
/// turn completes — the transition the live event pump will drive from
/// `TurnCompleted` in a later slice — it returns to `Idle` and does **not**
/// cancel the send. Routing it through Claude's `AwaitingEcho` path would
/// instead leave this successful send waiting for an echo that never comes, so
/// the turn end would requeue and re-type a message Codex has already accepted.
#[tokio::test]
async fn codex_turn_completing_does_not_cancel_its_send() {
    let factory = FakeAgentFactory::new("thr_fsm", Some("turn_fsm"));
    let ix = interactor_with_codex_factory(factory);

    let (send, _) = ix
        .enqueue_send(
            SendTarget::NewSession {
                pull_request_number: None,
                provider: AgentProvider::Codex,
                workdir: None,
                launch_option_ids: Vec::new(),
                worktree: None,
            },
            "go",
            None,
        )
        .await
        .unwrap();
    ix.await_launch().await;
    let session_id = send.session_id.clone();

    // The Codex turn is in flight, tracked with no send id.
    assert_eq!(
        ix.live_state_for(&session_id).await.turn,
        TurnState::InFlight { send_id: None },
    );
    // The send was already completed at the turn/start ack.
    assert_eq!(
        ix.store().send(send.id).await.unwrap().unwrap().status,
        SendStatus::Matched,
    );

    // The running turn completes (what the C3e-3 pump maps `TurnCompleted` to).
    let next = ix
        .apply_turn_input(&session_id, TurnInput::Stop)
        .await
        .unwrap();
    assert_eq!(next, TurnState::Idle, "the turn returns to idle");

    // Crucially, the completed send is NOT cancelled by the turn ending.
    assert_eq!(
        ix.store().send(send.id).await.unwrap().unwrap().status,
        SendStatus::Matched,
        "TurnCompleted → Stop must not cancel a Codex send (no echo correlation)"
    );
}
