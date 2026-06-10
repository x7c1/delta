use delta_model::SessionId;

use crate::interactor::testing::*;

/// The root fix: `open_session` catches the DB up to the existing transcript
/// before returning, so the resume's first prompt resolves thread context
/// against the user's real last thread instead of a DB-behind `None`. After the
/// open, the prior history is ingested and `latest_user_thread` reports the
/// prior user line's thread.
#[tokio::test]
async fn open_session_syncs_existing_transcript_so_latest_user_thread_is_known() {
    let ix = interactor();
    // Register a known-but-closed session (empty transcript at registration).
    ix.on_user_prompt_submit(submit_in(
        "sess-R",
        "/elsewhere/t.jsonl",
        "/elsewhere",
        "seed",
    ))
    .await
    .unwrap();
    let id = SessionId::from("sess-R");
    let main = ix.store().main_thread_id(&id).await.unwrap();

    // Prior history exists in the transcript but is not yet ingested (DB behind).
    ix.transcript_fake()
        .push_to("/elsewhere/t.jsonl", user_line("u-prior", "prior prompt"));
    ix.transcript_fake().push_to(
        "/elsewhere/t.jsonl",
        assistant_line("a-prior", "prior reply"),
    );
    assert!(
        ix.store().latest_user_thread(&id).await.unwrap().is_none(),
        "precondition: DB behind transcript"
    );

    // Resume the session. The catch-up sync runs as part of the open.
    ix.open_session(&id).await.unwrap();

    // The DB is now caught up: the prior user line is ingested and reported as
    // the latest user thread, so the first post-resume prompt sees the real
    // previous thread rather than `None`.
    assert_eq!(
        ix.store().latest_user_thread(&id).await.unwrap(),
        Some(main),
        "the prior user line is now the known latest user thread"
    );
    let view = ix.thread_view(main).await.unwrap();
    let uuids: Vec<&str> = view.iter().map(|m| m.uuid.as_str()).collect();
    assert!(
        uuids.contains(&"u-prior"),
        "prior user line ingested on open"
    );
    assert!(
        uuids.contains(&"a-prior"),
        "prior assistant line ingested on open"
    );
}
