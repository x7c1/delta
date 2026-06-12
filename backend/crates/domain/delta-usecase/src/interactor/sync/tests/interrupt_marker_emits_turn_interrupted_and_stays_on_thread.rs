use delta_model::{MessageUuid, SendStatus, SessionId};

use crate::interactor::testing::*;
use crate::ports::SessionEvent;

/// When the user interrupts the in-flight turn, Claude's `Stop` hook never
/// fires, so `TurnCompleted` is never emitted and the optimistic pending chip
/// would stay "in progress" forever. Claude does, however, write a discrete
/// `[Request interrupted by user]` user line to the transcript. Ingesting it
/// must: (a) emit `TurnInterrupted` so the browser clears the stuck pending,
/// (b) attribute the marker to the interrupted turn's thread (its `carry_thread`,
/// here the branch child), NOT reset to `main`, and (c) leave an unrelated
/// still-pending send untouched (not matched, not cancelled).
#[tokio::test]
async fn interrupt_marker_emits_turn_interrupted_and_stays_on_thread() {
    let ix = interactor();
    ix.seed_session().await;
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // Start a branch turn and match its user line onto the child thread, so
    // `carry_thread` is the child when the interrupt lands.
    let parent = MessageUuid::from("uuid-parent");
    let pending = ix
        .enqueue_send(branch_off(main, &parent), "branch text", None)
        .await
        .unwrap();
    let child = pending.thread_id;
    assert_ne!(child, main);
    ix.transcript_fake().push(user_line("u-b", "branch text"));
    ix.on_user_prompt_submit(submit("branch text"))
        .await
        .unwrap();

    // An unrelated, still-pending send is queued. Its text must not collide with
    // the interrupt marker, so the marker must not match or cancel it.
    let unrelated = ix
        .enqueue_send(to(main), "unrelated prompt", None)
        .await
        .unwrap();

    // The turn is interrupted mid-flight: Claude writes the marker line, which
    // the background tail ingests (no `Stop` hook fires).
    ix.transcript_fake().push(interrupt_line("u-int"));
    let (_groups, events) = ix.poll_transcript().await.unwrap();

    // (a) A `TurnInterrupted` is emitted for this session.
    assert!(
        events.iter().any(|e| matches!(
            e,
            SessionEvent::TurnInterrupted { session_id } if *session_id == session
        )),
        "ingesting the interrupt marker emits TurnInterrupted, got {events:?}"
    );

    // (b) The marker message is attributed to the interrupted turn's thread (the
    // branch child), not reset to `main`.
    let view = ix.thread_view(child).await.unwrap();
    assert!(
        view.iter().any(|m| m.uuid.as_str() == "u-int"),
        "the interrupt marker stays on the interrupted turn's thread"
    );
    let main_view = ix.thread_view(main).await.unwrap();
    assert!(
        !main_view.iter().any(|m| m.uuid.as_str() == "u-int"),
        "the interrupt marker must not leak onto main"
    );

    // (c) The unrelated pending send is left untouched (still pending).
    let head = ix
        .store()
        .head_dispatched_send(&session)
        .await
        .unwrap()
        .expect("the unrelated send is still pending");
    assert_eq!(head.id, unrelated.id);
    assert_eq!(head.status, SendStatus::Dispatched);
}
