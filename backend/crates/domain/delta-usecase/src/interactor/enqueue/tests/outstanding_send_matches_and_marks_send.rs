//! The ordinary path, where position and text agree: the prompt that submitted
//! consumes the outstanding send (position) *and* its transcript line reads
//! back as that send's own text (attribution), so the send is bound to a real
//! message uuid and the turn is announced on the thread it was composed for.
//!
//! This is the full outcome a rewritten echo cannot reach: that one is consumed
//! all the same, but settles as delivered with no uuid — see
//! `turn_end_settles_a_consumed_send_as_delivered`.

use delta_model::{MessageUuid, SendStatus, SessionId};

use crate::interactor::context::{frame_branch_entry_context, frame_locator_context};
use crate::interactor::testing::*;
use crate::ports::SessionEvent;

#[tokio::test]
async fn outstanding_send_matches_and_marks_send() {
    let ix = interactor();
    // Register and obtain main thread, idle (the registration turn completed).
    ix.seed_session().await;
    let main = ix
        .store()
        .main_thread_id(&SessionId::from("sess-1"))
        .await
        .unwrap();

    // Queue a send (also dispatches to fake tmux).
    let (send, _) = ix
        .enqueue_send(to(main), "hello world", Some("[quote]"))
        .await
        .unwrap();
    assert_eq!(send.status, SendStatus::Dispatched);

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
                send_id,
                ..
            } => Some((matched_uuid.clone(), *send_id)),
            _ => None,
        })
        .expect("turn started event");
    assert_eq!(started.0, MessageUuid::from("uuid-1"));
    assert_eq!(started.1, send.id);

    // Marked matched; no longer outstanding.
    let head = ix
        .store()
        .head_dispatched_send(&SessionId::from("sess-1"))
        .await
        .unwrap();
    assert!(head.is_none());
}
