//! `MessageDisplay` accumulates the in-flight turn's assistant text into a
//! provisional live preview and emits `AssistantStreaming`, and the preview is
//! cleared when the turn ends (Stop or interrupt) so the persisted message
//! takes over without a duplicate.

use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::ports::{MessageDisplayHook, SessionEvent, StopHook};
use crate::turn::TurnInput;

fn display(message_id: &str, index: u32, final_: bool, delta: &str) -> MessageDisplayHook {
    MessageDisplayHook {
        session_id: SessionId::from("sess-1"),
        message_id: message_id.to_owned(),
        index,
        final_,
        delta: delta.to_owned(),
    }
}

#[tokio::test]
async fn deltas_accumulate_into_the_live_preview_and_emit_assistant_streaming() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");
    let main_thread = ix.store().main_thread_id(&session).await.unwrap();

    // The first chunk emits an AssistantStreaming carrying just that chunk,
    // attributed to the in-flight turn's thread (the seed prompt's thread).
    let events = ix
        .on_message_display(display("msg-1", 0, false, "Hel"))
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    match &events[0] {
        SessionEvent::AssistantStreaming {
            session_id,
            thread_id,
            message_id,
            index,
            final_,
            delta,
        } => {
            assert_eq!(session_id, &session);
            assert_eq!(*thread_id, main_thread);
            assert_eq!(message_id, "msg-1");
            assert_eq!(*index, 0);
            assert!(!*final_);
            assert_eq!(delta, "Hel");
        }
        other => panic!("expected AssistantStreaming, got {other:?}"),
    }

    ix.on_message_display(display("msg-1", 1, false, "lo "))
        .await
        .unwrap();
    ix.on_message_display(display("msg-1", 2, true, "world"))
        .await
        .unwrap();

    // The buffer holds the full message, joined in index order, for the
    // in-flight thread, marked final.
    let preview = ix
        .with_runtime(&session, |state| {
            state.streaming_message().map(|s| {
                (
                    s.message_id.clone(),
                    s.thread_id,
                    s.text(),
                    s.final_,
                )
            })
        })
        .await
        .expect("a message is streaming");
    assert_eq!(preview.0, "msg-1");
    assert_eq!(preview.1, main_thread);
    assert_eq!(preview.2, "Hello world");
    assert!(preview.3);
}

#[tokio::test]
async fn out_of_order_chunks_are_joined_in_index_order() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    ix.on_message_display(display("msg-1", 1, false, "B"))
        .await
        .unwrap();
    ix.on_message_display(display("msg-1", 0, false, "A"))
        .await
        .unwrap();
    ix.on_message_display(display("msg-1", 2, true, "C"))
        .await
        .unwrap();

    let text = ix
        .with_runtime(&session, |state| {
            state.streaming_message().map(|s| s.text())
        })
        .await
        .expect("a message is streaming");
    assert_eq!(text, "ABC");
}

#[tokio::test]
async fn turn_completed_clears_the_live_preview() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    ix.on_message_display(display("msg-1", 0, false, "partial"))
        .await
        .unwrap();
    assert!(
        ix.with_runtime(&session, |state| state.streaming_message().is_some())
            .await,
        "the preview exists while the turn is in flight"
    );

    ix.on_stop(StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();

    assert!(
        !ix.with_runtime(&session, |state| state.streaming_message().is_some())
            .await,
        "the turn ending drops the preview so the persisted message takes over"
    );
}

#[tokio::test]
async fn an_interrupt_clears_the_live_preview() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    ix.on_message_display(display("msg-1", 0, false, "partial"))
        .await
        .unwrap();

    // An interrupt ends the turn the same way a Stop does (the transcript
    // sync feeds Interrupt into the turn machine); seed that transition.
    ix.apply_turn_input(&session, TurnInput::Interrupt)
        .await
        .unwrap();

    assert!(
        !ix.with_runtime(&session, |state| state.streaming_message().is_some())
            .await,
        "an interrupt drops the preview just like a completed turn"
    );
}
