//! The boot restore holds every send the previous process left `dispatched`.
//! When the pane survived the restart, its keystrokes may have been submitted
//! there; the re-adoption's transcript catch-up is where that shows, and such a
//! send is settled as delivered instead of being left "held — send or cancel",
//! where pressing Send would type it twice. Anything the catch-up does not
//! clearly show stays held.

use delta_model::{MessageUuid, SendStatus};

use crate::interactor::testing::*;

use super::readoption_support::{left_behind, survives, SURVIVING_TOKEN};

#[tokio::test]
async fn a_held_send_whose_prompt_the_catch_up_shows_is_settled() {
    let ix = interactor();
    let id = left_behind(&ix, "sess-A", SURVIVING_TOKEN).await;
    let main = ix.store().main_thread_id(&id).await.unwrap();
    let delivered = ix
        .store()
        .enqueue_send(&id, main, None, "fix the build", None)
        .await
        .unwrap();
    let swallowed = ix
        .store()
        .enqueue_send(&id, main, None, "then run the tests", None)
        .await
        .unwrap();
    // The previous process died with both outstanding; the next one holds them.
    ix.store().restore_all_dispatched().await.unwrap();
    survives(&ix, SURVIVING_TOKEN);
    // The surviving pane submitted the first one (surrounding whitespace aside,
    // the line says exactly what Delta typed) but never saw the second.
    ix.transcript_fake().push_to(
        "/work/sess-A.jsonl",
        user_line("prompt-line", "fix the build\n"),
    );

    ix.readopt_surviving_sessions(false).await.unwrap();

    let row = ix.store().send(delivered.id).await.unwrap().expect("row");
    assert_eq!(row.status, SendStatus::Matched);
    assert_eq!(row.matched_uuid, Some(MessageUuid::from("prompt-line")));
    assert_eq!(row.held_at, None);

    let row = ix.store().send(swallowed.id).await.unwrap().expect("row");
    assert_eq!(
        row.status,
        SendStatus::Queued,
        "not shown, so left for the user"
    );
    assert!(row.held_at.is_some());
    assert!(
        ix.tmux_fake().sent.lock().unwrap().is_empty(),
        "settling types nothing"
    );
}

/// A matching text is not enough on its own: a line older than the send row
/// cannot be its echo, and one line answers one send only.
#[tokio::test]
async fn a_line_older_than_the_send_or_already_claimed_settles_nothing() {
    let ix = interactor();
    let id = left_behind(&ix, "sess-A", SURVIVING_TOKEN).await;
    let main = ix.store().main_thread_id(&id).await.unwrap();
    // Two held sends with the same prompt; the fake stamps both rows
    // `2026-01-01T00:00:00Z`.
    let first = ix
        .store()
        .enqueue_send(&id, main, None, "continue", None)
        .await
        .unwrap();
    let second = ix
        .store()
        .enqueue_send(&id, main, None, "continue", None)
        .await
        .unwrap();
    ix.store().restore_all_dispatched().await.unwrap();
    survives(&ix, SURVIVING_TOKEN);
    // One line written before the rows existed (the user typed the same words
    // earlier), and one that is the echo of the first send.
    ix.transcript_fake().push_to(
        "/work/sess-A.jsonl",
        crate::ports::TranscriptMessage {
            created_at: Some("2025-12-31T23:59:59Z".into()),
            ..user_line("earlier-line", "continue")
        },
    );
    ix.transcript_fake()
        .push_to("/work/sess-A.jsonl", user_line("echo-line", "continue"));

    ix.readopt_surviving_sessions(false).await.unwrap();

    let row = ix.store().send(first.id).await.unwrap().expect("row");
    assert_eq!(row.status, SendStatus::Matched);
    assert_eq!(row.matched_uuid, Some(MessageUuid::from("echo-line")));
    let row = ix.store().send(second.id).await.unwrap().expect("row");
    assert_eq!(
        row.status,
        SendStatus::Queued,
        "the only later line went to the older send"
    );
    assert!(row.held_at.is_some());
}
