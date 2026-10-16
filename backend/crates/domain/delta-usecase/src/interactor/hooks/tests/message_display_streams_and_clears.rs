//! Each `MessageDisplay` chunk is forwarded as one `AssistantStreaming` event
//! carrying the chunk's body, index and final flag, attributed to the in-flight
//! turn's thread; the browser accumulates the preview. After the turn ends
//! (Stop or interrupt) the next chunk is attributed to the new turn and indexed
//! as a fresh message.

use delta_model::{SessionId, ThreadId};

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

/// One emitted `AssistantStreaming`, flattened for comparison.
#[derive(Debug, PartialEq, Eq)]
struct Streamed {
    thread_id: ThreadId,
    message_id: String,
    index: u32,
    final_: bool,
    delta: String,
}

fn streamed(
    thread_id: ThreadId,
    message_id: &str,
    index: u32,
    final_: bool,
    delta: &str,
) -> Streamed {
    Streamed {
        thread_id,
        message_id: message_id.to_owned(),
        index,
        final_,
        delta: delta.to_owned(),
    }
}

/// Feed one chunk and return the single `AssistantStreaming` it emits.
async fn stream(ix: &TestInteractor, hook: MessageDisplayHook) -> Streamed {
    let events = ix.on_message_display(hook).await.unwrap();
    match events.as_slice() {
        [SessionEvent::AssistantStreaming {
            session_id,
            thread_id,
            message_id,
            index,
            final_,
            delta,
        }] => {
            assert_eq!(session_id, &SessionId::from("sess-1"));
            Streamed {
                thread_id: *thread_id,
                message_id: message_id.clone(),
                index: *index,
                final_: *final_,
                delta: delta.clone(),
            }
        }
        other => panic!("expected a single AssistantStreaming, got {other:?}"),
    }
}

#[tokio::test]
async fn each_chunk_is_emitted_with_its_body_index_and_final_flag() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    let emitted = vec![
        stream(&ix, display("msg-1", 0, false, "Hel")).await,
        stream(&ix, display("msg-1", 1, false, "lo ")).await,
        stream(&ix, display("msg-1", 2, true, "world")).await,
    ];

    assert_eq!(
        emitted,
        vec![
            streamed(main, "msg-1", 0, false, "Hel"),
            streamed(main, "msg-1", 1, false, "lo "),
            streamed(main, "msg-1", 2, true, "world"),
        ],
        "every chunk is forwarded as-is on the in-flight turn's thread"
    );
}

#[tokio::test]
async fn a_chunk_after_stop_is_a_fresh_message_on_the_next_turn() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    stream(&ix, display("msg-1", 0, false, "first ")).await;
    stream(&ix, display("msg-1", 1, true, "reply")).await;

    ix.on_stop(StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();

    ix.on_user_prompt_submit(submit("again")).await.unwrap();
    assert_eq!(
        stream(&ix, display("msg-2", 0, false, "second")).await,
        streamed(main, "msg-2", 0, false, "second"),
        "the next turn's first chunk starts a fresh message at index 0"
    );
}

#[tokio::test]
async fn a_chunk_after_an_interrupt_is_a_fresh_message_on_the_next_turn() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    stream(&ix, display("msg-1", 0, false, "partial")).await;

    // An interrupt ends the turn the same way a Stop does (the transcript
    // sync feeds Interrupt into the turn machine); seed that transition.
    ix.apply_turn_input(&session, TurnInput::Interrupt)
        .await
        .unwrap();

    ix.on_user_prompt_submit(submit("again")).await.unwrap();
    assert_eq!(
        stream(&ix, display("msg-2", 0, false, "retry")).await,
        streamed(main, "msg-2", 0, false, "retry"),
        "the next turn's first chunk starts a fresh message at index 0"
    );
}
