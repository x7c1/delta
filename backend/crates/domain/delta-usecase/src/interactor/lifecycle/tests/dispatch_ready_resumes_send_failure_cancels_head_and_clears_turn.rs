use std::time::Instant;

use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::interactor::session_actor::runtime::RESUME_DISPATCH_SETTLE;
use crate::ports::SessionStore;

/// When the tick dispatch's `send_line` fails, the held first prompt's pending
/// row is cancelled and the turn returns to idle, mirroring the other dispatch
/// sites so a failed dispatch cannot wedge the queue. The resume is still
/// removed from the resuming map (it was drained before the send).
#[tokio::test]
async fn dispatch_ready_resumes_send_failure_cancels_head_and_clears_turn() {
    // An interactor whose tmux `send_line` always fails.
    let ix = interactor_with_failing_tmux();
    let ready_at = Instant::now();
    let session_id = SessionId::from("sess-send-fail");

    // Register the session, write its held first prompt's pending row, and mark
    // the turn active (the keystroke was held, not its bookkeeping).
    ix.on_user_prompt_submit(submit_in(
        session_id.as_str(),
        "/elsewhere/t.jsonl",
        "/elsewhere",
        "seed",
    ))
    .await
    .unwrap();
    let main = ix.store().main_thread_id(&session_id).await.unwrap();
    let held = ix
        .store()
        .enqueue_send(&session_id, main, None, "held prompt", None)
        .await
        .unwrap();
    ix.apply_turn_input(
        &session_id,
        crate::turn::TurnInput::Dispatch { send_id: held.id },
    )
    .await
    .unwrap();

    // Mark it resuming-but-not-ready with a held prompt, then ready at `ready_at`.
    ix.push_resuming_at(
        "delta-8",
        &session_id,
        Some("held prompt".to_owned()),
        ready_at,
    )
    .await;
    assert!(ix.mark_resume_ready_at(&session_id, ready_at).await);

    // Dispatch once settled: `send_line` fails inside the tick.
    ix.dispatch_ready_resumes(ready_at + RESUME_DISPATCH_SETTLE)
        .await
        .unwrap();

    // The undeliverable pending row was cancelled (no longer the pending head)...
    let head = ix.store().head_dispatched_send(&session_id).await.unwrap();
    assert!(
        head.is_none(),
        "the head pending send was cancelled on dispatch failure"
    );
    // ...the turn returned to idle so a later send is not stranded behind it...
    assert_eq!(
        ix.turn_state_for(&session_id).await,
        crate::turn::TurnState::Idle,
        "the turn returned to idle on dispatch failure"
    );
    // ...and the resume left the resuming map (it was drained before the send).
    assert!(ix.resuming_session_ids().await.is_empty());
}
