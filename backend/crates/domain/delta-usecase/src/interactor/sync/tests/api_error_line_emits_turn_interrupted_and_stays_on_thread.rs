use delta_model::{MessageUuid, SendStatus, SessionId};

use crate::interactor::testing::*;
use crate::ports::SessionEvent;

/// When a turn ends on an API error (a usage/session limit, a rate limit, or any
/// other API failure), Claude's `Stop` hook never fires and no interrupt marker
/// is written — only a synthetic `isApiErrorMessage` assistant line. Without a
/// transcript-driven fallback the turn machine would stay in flight forever and
/// the optimistic pending chip would never clear. Ingesting the api-error line
/// must: (a) emit `TurnInterrupted` so the browser clears the stuck pending,
/// (b) attribute the line to the in-flight turn's thread (its `carry_thread`,
/// here the branch child), NOT reset to `main`, and (c) leave an unrelated
/// still-pending send untouched (not matched, not cancelled).
#[tokio::test]
async fn api_error_line_emits_turn_interrupted_and_stays_on_thread() {
    let ix = interactor();
    ix.seed_session().await;
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // Start a branch turn and match its user line onto the child thread, so
    // `carry_thread` is the child when the api-error line lands.
    let parent = MessageUuid::from("uuid-parent");
    let (pending, _) = ix
        .enqueue_send(branch_off(main, &parent), "branch text", None)
        .await
        .unwrap();
    let child = pending.thread_id;
    assert_ne!(child, main);
    ix.transcript_fake().push(user_line("u-b", "branch text"));
    ix.on_user_prompt_submit(submit("branch text"))
        .await
        .unwrap();

    // An unrelated, still-pending send is queued. The api-error line is an
    // assistant line carrying no author text, so it must not match or cancel it.
    let (unrelated, _) = ix
        .enqueue_send(to(main), "unrelated prompt", None)
        .await
        .unwrap();

    // The turn hits a usage/session limit mid-flight: Claude writes the
    // synthetic `isApiErrorMessage` line, which the background tail ingests (no
    // `Stop` hook fires).
    ix.transcript_fake().push(api_error_line("u-api-error"));
    let (_groups, events) = ix.poll_transcript().await.unwrap();

    // (a) A `TurnInterrupted` is emitted for this session (the reused
    // browser/flush signal for a hook-independent turn-end).
    assert!(
        events.iter().any(|e| matches!(
            e,
            SessionEvent::TurnInterrupted { session_id } if *session_id == session
        )),
        "ingesting the api-error line emits TurnInterrupted, got {events:?}"
    );

    // (b) The api-error message is attributed to the in-flight turn's thread (the
    // branch child), not reset to `main`.
    let view = ix.thread_view(child).await.unwrap();
    assert!(
        view.iter().any(|m| m.uuid.as_str() == "u-api-error"),
        "the api-error line stays on the in-flight turn's thread"
    );
    let main_view = ix.thread_view(main).await.unwrap();
    assert!(
        !main_view.iter().any(|m| m.uuid.as_str() == "u-api-error"),
        "the api-error line must not leak onto main"
    );

    // (c) The unrelated pending send is left untouched (still pending). It was
    // released and re-dispatched by the turn-end flush, so its status is now
    // `dispatched` and it is the head outstanding send — never matched against
    // or cancelled by the api-error line.
    let head = ix
        .store()
        .head_dispatched_send(&session)
        .await
        .unwrap()
        .expect("the unrelated send is dispatched after the turn-end flush");
    assert_eq!(head.id, unrelated.id);
    assert_eq!(head.status, SendStatus::Dispatched);
}
