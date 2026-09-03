use delta_model::SessionId;

use super::listed_ids;
use crate::interactor::testing::*;

/// The session list is open-first: a session with a live pane leads a closed one
/// however much more recently the closed one was active. Closing it drops it
/// back to its recency position — nothing else about the order changes.
#[tokio::test]
async fn list_sessions_page_lists_open_sessions_before_closed_ones() {
    let ix = interactor();

    ix.on_user_prompt_submit(submit_for("sess-open", "/tmp/open.jsonl", "seed"))
        .await
        .unwrap();
    ix.on_user_prompt_submit(submit_for("sess-closed", "/tmp/closed.jsonl", "seed"))
        .await
        .unwrap();
    // Bind both so the transcript tail (scoped to open sessions) ingests the
    // seeded growth below; `sess-closed` is closed again once it has activity.
    ix.bind_open_session("delta-open", &SessionId::from("sess-open"))
        .await;
    ix.bind_open_session("delta-closed", &SessionId::from("sess-closed"))
        .await;
    ix.transcript_fake().push_to(
        "/tmp/open.jsonl",
        assistant_line_at("a-open", "older", "2026-01-01T00:00:00Z"),
    );
    ix.transcript_fake().push_to(
        "/tmp/closed.jsonl",
        assistant_line_at("a-closed", "newer", "2026-02-01T00:00:00Z"),
    );
    ix.poll_transcript().await.unwrap();
    ix.close_session(&SessionId::from("sess-closed"))
        .await
        .unwrap();

    // On recency alone `sess-closed` would lead; being closed puts it second.
    let page = ix.list_sessions_page(None, 30).await.unwrap();
    assert_eq!(listed_ids(&page), vec!["sess-open", "sess-closed"]);
    assert!(page.listings[0].open, "the leading session is the live one");

    // Closing the open session leaves nothing live, so pure recency returns.
    ix.close_session(&SessionId::from("sess-open"))
        .await
        .unwrap();
    let page = ix.list_sessions_page(None, 30).await.unwrap();
    assert_eq!(
        listed_ids(&page),
        vec!["sess-closed", "sess-open"],
        "a closed session falls back to its recency position"
    );
}
