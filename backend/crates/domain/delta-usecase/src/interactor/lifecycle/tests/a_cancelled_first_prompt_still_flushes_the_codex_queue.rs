//! A Codex launch whose first prompt was cancelled mid-flight still delivers
//! the sends that were queued behind it, at the bind.
//!
//! The ordinary adapter-backed queue drains at a turn end: the bind dispatches
//! the first prompt, and `turn/completed` flushes whatever queued behind it
//! (`send_during_a_codex_launch_is_queued` pins that path). Cancelling the
//! first prompt during the launch window removes the very turn that flush hangs
//! off — `dispatch_first_agent_prompt` finds the row no longer `queued` and
//! starts nothing — so nothing else would ever move the queue and the later row
//! would sit `queued` until the user sent again. The flush the bind runs after
//! registering the session is what closes that hole, and the turn-idle guard
//! keeps it to exactly one dispatch on the ordinary path.

use delta_model::{AgentProvider, SendStatus};

use crate::interactor::testing::*;
use crate::SendTarget;

#[tokio::test]
async fn a_cancelled_first_prompt_still_flushes_the_codex_queue() {
    let gate = ConnectGate::closed();
    let factory = FakeAgentFactory::gated("thr_gated", Some("turn_gated"), &gate);
    let ix = interactor_with_codex_factory(factory.clone());

    let (first, _) = ix
        .enqueue_send(
            SendTarget::NewSession {
                pull_request_number: None,
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

    let main = ix.store().main_thread_id(&session_id).await.unwrap();
    let (queued, _) = ix
        .enqueue_send(to(main), "and one more while it starts", None)
        .await
        .expect("a plain send to a still-starting Codex session is accepted");
    assert_eq!(queued.status, SendStatus::Queued);

    // The user drops the opening prompt while the launch is still held — a
    // `queued` row is cancellable throughout the spawning window.
    ix.cancel_send(first.id)
        .await
        .expect("a queued first prompt can be cancelled while the launch runs");
    assert_eq!(
        ix.store().send(first.id).await.unwrap().unwrap().status,
        SendStatus::Cancelled,
    );

    // Release the launch. There is no first prompt left to dispatch, so no turn
    // starts and no `turn/completed` is coming: the bind's own flush is the
    // only thing that can deliver the surviving row.
    gate.open();
    ix.await_launch().await;
    assert!(
        ix.is_session_open(&session_id).await,
        "the released launch bound its agent"
    );
    assert_eq!(
        factory.log().lock().unwrap().sends.clone(),
        vec!["and one more while it starts".to_owned()],
        "the surviving queued send reached the adapter exactly once, and the \
         cancelled first prompt was not resurrected"
    );
    assert!(
        ix.store().open_sends(&session_id).await.unwrap().is_empty(),
        "the flushed row completed at its turn/start acknowledgement, and the \
         cancelled row is terminal"
    );
}
