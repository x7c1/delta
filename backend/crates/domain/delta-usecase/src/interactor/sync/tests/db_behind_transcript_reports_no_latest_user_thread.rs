use delta_model::SessionId;

use crate::interactor::testing::*;

/// Reproduces the DB-behind precondition that produced the resume bug: a known
/// session whose transcript already holds prior user history, but whose DB
/// message rows and read cursor have not caught up to it yet (a cold/just-
/// restored DB, or any DB-behind-transcript state). In that state
/// `latest_user_thread` reports `None`, even though the user really was in a
/// thread — the stale value that mis-seeds thread context on the first
/// post-resume prompt.
#[tokio::test]
async fn db_behind_transcript_reports_no_latest_user_thread() {
    let ix = interactor();
    // Register a known-but-closed session. At registration its transcript is
    // empty, so the cursor is 0 and no message rows exist.
    ix.on_user_prompt_submit(submit_in(
        "sess-R",
        "/elsewhere/t.jsonl",
        "/elsewhere",
        "seed",
    ))
    .await
    .unwrap();
    let id = SessionId::from("sess-R");

    // Prior history is written to the transcript WITHOUT syncing: the DB is now
    // behind the transcript (message table empty, cursor 0).
    ix.transcript_fake()
        .push_to("/elsewhere/t.jsonl", user_line("u-prior", "prior prompt"));
    ix.transcript_fake().push_to(
        "/elsewhere/t.jsonl",
        assistant_line("a-prior", "prior reply"),
    );

    // Precondition: the DB-behind state makes `latest_user_thread` report `None`
    // even though the transcript holds a prior user line.
    assert!(
        ix.store().latest_user_thread(&id).await.unwrap().is_none(),
        "DB behind transcript: no user row yet, so the latest user thread is unknown"
    );
    assert_eq!(
        ix.store().message_count(&id).await.unwrap(),
        0,
        "no prior history ingested yet"
    );
}
