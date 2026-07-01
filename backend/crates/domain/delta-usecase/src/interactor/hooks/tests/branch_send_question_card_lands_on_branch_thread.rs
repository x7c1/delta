//! A mid-turn branch send drives a new turn whose `AskUserQuestion` must be
//! attributed to the new branch thread — even though the branch's user line is
//! not in the transcript yet when the question fires.
//!
//! This reproduces the thread-resolution race fixed by
//! `SessionStore::in_progress_turn_thread`: before the fix the question fell
//! back to the latest persisted user thread (the prior turn's thread, or main),
//! so the browser's thread-scoped question card never matched the branch the
//! user is viewing and the card was hidden everywhere visible.

use delta_model::{MessageUuid, SendStatus, SessionId};

use crate::interactor::testing::*;
use crate::ports::SessionEvent;

const QUESTION_INPUT: &str = r#"{"questions":[{"question":"Which?","header":"Pick","options":[{"label":"A","description":"first"},{"label":"B","description":"second"}],"multiSelect":false}]}"#;

#[tokio::test]
async fn ask_user_question_in_a_just_dispatched_branch_turn_lands_on_the_branch_thread() {
    let ix = interactor();
    let session = SessionId::from("sess-1");
    ix.seed_session().await;
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // A first turn is in flight (its echo arrives), so the next send queues.
    ix.enqueue_send(to(main), "first", None).await.unwrap();
    ix.transcript_fake().push(user_line("u-first", "first"));
    ix.on_user_prompt_submit(submit("first")).await.unwrap();

    // A branch send arrives mid-turn: it eagerly creates a new child thread but
    // is held back as `queued` (no keystrokes typed into the busy pane).
    let parent = MessageUuid::from("uuid-parent");
    let (queued, _) = ix
        .enqueue_send(branch_off(main, &parent), "branch text", Some("quote"))
        .await
        .unwrap();
    assert_eq!(queued.status, SendStatus::Queued);
    let child = queued.thread_id;
    assert_ne!(child, main, "the branch child thread is created up front");

    // The first turn completes at Stop, which dispatches the queued branch send
    // (it becomes the head `dispatched` send) and types its keystrokes. Crucially
    // the branch's own user line is NOT ingested here: the later sync attributes
    // it. So the latest persisted user thread is still `main` (the seed/first
    // turn), NOT the branch.
    ix.on_stop(crate::ports::StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();
    assert_eq!(
        ix.store().latest_user_thread(&session).await.unwrap(),
        Some(main),
        "the branch user line is not ingested yet, so latest-user is still main"
    );

    // Claude Code now fires `AskUserQuestion` for the branch prompt, before the
    // branch user line appears in the JSONL. The question must be attributed to
    // the branch thread (the in-flight send's thread), not main.
    let events = ix
        .on_pre_tool_use(
            &session,
            "AskUserQuestion",
            QUESTION_INPUT,
            "toolu_q1",
            SEED_TRANSCRIPT_PATH,
        )
        .await
        .unwrap();

    match events.as_slice() {
        [SessionEvent::QuestionAsked { thread_id, .. }] => {
            assert_eq!(
                *thread_id, child,
                "the question lands on the new branch thread, not the prior turn's thread"
            );
            assert_ne!(*thread_id, main, "and certainly not on main");
        }
        other => panic!("expected a single QuestionAsked, got {other:?}"),
    }

    // The queryable live state mirrors the same branch attribution.
    let pending = ix
        .live_state_for(&session)
        .await
        .pending_question
        .expect("the question is queryable while it awaits an answer");
    assert_eq!(pending.thread_id, child);
}

#[tokio::test]
async fn streaming_preview_in_a_just_dispatched_branch_turn_lands_on_the_branch_thread() {
    let ix = interactor();
    let session = SessionId::from("sess-1");
    ix.seed_session().await;
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // First turn in flight; the next send queues.
    ix.enqueue_send(to(main), "first", None).await.unwrap();
    ix.transcript_fake().push(user_line("u-first", "first"));
    ix.on_user_prompt_submit(submit("first")).await.unwrap();

    // A mid-turn branch send: queued, with a new child thread.
    let parent = MessageUuid::from("uuid-parent");
    let (queued, _) = ix
        .enqueue_send(branch_off(main, &parent), "branch text", Some("quote"))
        .await
        .unwrap();
    let child = queued.thread_id;
    assert_ne!(child, main);

    // Stop dispatches the queued branch send (head `dispatched`); its user line
    // is not ingested yet.
    ix.on_stop(crate::ports::StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();

    // The branch turn's assistant text streams in before the user line lands.
    // The preview must be attributed to the branch thread.
    let events = ix
        .on_message_display(crate::ports::MessageDisplayHook {
            session_id: session.clone(),
            message_id: "msg-branch".to_owned(),
            index: 0,
            final_: false,
            delta: "partial".to_owned(),
        })
        .await
        .unwrap();

    match events.as_slice() {
        [SessionEvent::AssistantStreaming { thread_id, .. }] => {
            assert_eq!(
                *thread_id, child,
                "the streaming preview lands on the new branch thread"
            );
            assert_ne!(*thread_id, main);
        }
        other => panic!("expected a single AssistantStreaming, got {other:?}"),
    }

    // The buffered preview carries the same branch attribution.
    let preview_thread = ix
        .with_runtime(&session, |state| {
            state.streaming_message().map(|s| s.thread_id)
        })
        .await
        .expect("a message is streaming");
    assert_eq!(preview_thread, child);
}
