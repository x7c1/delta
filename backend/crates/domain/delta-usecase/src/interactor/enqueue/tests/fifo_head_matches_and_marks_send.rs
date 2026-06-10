use delta_model::{MessageUuid, PendingSendStatus, SessionId};

use crate::interactor::context::{frame_branch_entry_context, frame_locator_context};
use crate::interactor::testing::*;
use crate::ports::SessionEvent;

#[tokio::test]
async fn fifo_head_matches_and_marks_send() {
    let ix = interactor();
    // Register and obtain main thread.
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let main = ix
        .store()
        .main_thread_id(&SessionId::from("sess-1"))
        .await
        .unwrap();

    // Queue a send (also dispatches to fake tmux).
    let pending = ix
        .enqueue_send(to(main), "hello world", Some("[quote]"))
        .await
        .unwrap();
    assert_eq!(pending.status, PendingSendStatus::Pending);

    // The transcript now contains the matching user line.
    ix.transcript_fake()
        .push(user_line("uuid-1", "hello world"));

    let (events, additional) = ix
        .on_user_prompt_submit(submit("hello world"))
        .await
        .unwrap();
    // A locator quote → first entry into a branch: the locator frame plus a
    // note binding the quote to the thread the send targets.
    let expected = frame_branch_entry_context(&frame_locator_context("[quote]").unwrap(), main);
    assert_eq!(additional, Some(expected));
    let started = events
        .iter()
        .find_map(|e| match e {
            SessionEvent::TurnStarted {
                matched_uuid,
                pending_send_id,
                ..
            } => Some((matched_uuid.clone(), *pending_send_id)),
            _ => None,
        })
        .expect("turn started event");
    assert_eq!(started.0, MessageUuid::from("uuid-1"));
    assert_eq!(started.1, pending.id);

    // Marked matched; no longer the head.
    let head = ix
        .store()
        .head_pending_send(&SessionId::from("sess-1"))
        .await
        .unwrap();
    assert!(head.is_none());
}
