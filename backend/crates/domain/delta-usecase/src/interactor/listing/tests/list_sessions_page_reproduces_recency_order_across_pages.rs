use delta_model::SessionId;

use crate::interactor::testing::*;

/// Paging across two pages reproduces the single-shot recency order of
/// `list_sessions_orders_by_most_recent_activity`: most recent first, with a
/// message-less session falling back to its `created_at`.
#[tokio::test]
async fn list_sessions_page_reproduces_recency_order_across_pages() {
    let ix = interactor();

    ix.on_user_prompt_submit(submit_for("sess-old", "/tmp/old.jsonl", "seed"))
        .await
        .unwrap();
    ix.on_user_prompt_submit(submit_for("sess-new", "/tmp/new.jsonl", "seed"))
        .await
        .unwrap();
    ix.on_user_prompt_submit(submit_for("sess-quiet", "/tmp/quiet.jsonl", "seed"))
        .await
        .unwrap();
    // Open each session so the tail (scoped to open sessions) ingests their
    // seeded transcript growth below; the listing sort itself reads the store
    // regardless of open/closed.
    ix.bind_open_session("delta-old", &SessionId::from("sess-old"))
        .await;
    ix.bind_open_session("delta-new", &SessionId::from("sess-new"))
        .await;
    ix.bind_open_session("delta-quiet", &SessionId::from("sess-quiet"))
        .await;
    ix.transcript_fake().push_to(
        "/tmp/old.jsonl",
        assistant_line_at("a-old", "older", "2025-12-31T00:00:00Z"),
    );
    ix.transcript_fake().push_to(
        "/tmp/new.jsonl",
        assistant_line_at("a-new", "newer", "2026-02-01T00:00:00Z"),
    );
    ix.poll_transcript().await.unwrap();

    // Page through two at a time; concatenating the pages yields the same order
    // the all-at-once method asserts.
    let first = ix.list_sessions_page(None, 2).await.unwrap();
    let first_ids: Vec<_> = first
        .listings
        .iter()
        .map(|l| l.session.id.as_str().to_owned())
        .collect();
    assert_eq!(first_ids, vec!["sess-new", "sess-quiet"]);
    assert!(first.next.is_some(), "a full page yields a cursor");

    let second = ix.list_sessions_page(first.next, 2).await.unwrap();
    let second_ids: Vec<_> = second
        .listings
        .iter()
        .map(|l| l.session.id.as_str().to_owned())
        .collect();
    assert_eq!(second_ids, vec!["sess-old"]);
    assert!(
        second.next.is_none(),
        "a short last page yields no further cursor"
    );
}
