use delta_model::SessionId;

use crate::interactor::testing::*;

/// `on_stop` routes by the hook's own session id: a `Stop` for one session syncs
/// only that session's transcript, leaving the other session untouched.
#[tokio::test]
async fn on_stop_routes_sync_by_hook_session_id() {
    let ix = interactor();

    // Register two sessions, each with its own transcript path.
    ix.on_user_prompt_submit(submit_for("sess-1", "/tmp/s1.jsonl", "seed"))
        .await
        .unwrap();
    ix.on_user_prompt_submit(submit_for("sess-2", "/tmp/s2.jsonl", "seed"))
        .await
        .unwrap();

    // Each session's transcript grows by one assistant line, on its own path.
    ix.transcript_fake()
        .push_to("/tmp/s1.jsonl", assistant_line("a-1", "reply one"));
    ix.transcript_fake()
        .push_to("/tmp/s2.jsonl", assistant_line("a-2", "reply two"));

    // A `Stop` for sess-1 must ingest only sess-1's line.
    ix.on_stop(crate::ports::StopHook {
        session_id: SessionId::from("sess-1"),
        stop_reason: None,
    })
    .await
    .unwrap();

    assert_eq!(
        ix.store()
            .message_count(&SessionId::from("sess-1"))
            .await
            .unwrap(),
        1,
        "the Stop for sess-1 ingested its assistant line"
    );
    assert_eq!(
        ix.store()
            .message_count(&SessionId::from("sess-2"))
            .await
            .unwrap(),
        0,
        "sess-2 was not synced by a Stop addressed to sess-1"
    );
}
