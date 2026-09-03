use delta_model::SessionId;

use super::listed_ids;
use crate::interactor::testing::*;

/// Paging across two pages reproduces the single-shot recency order of
/// `list_sessions_orders_by_most_recent_activity`: most recent first, with a
/// message-less session falling back to its `created_at`.
///
/// Every session is closed before the walk so the open-first grouping is inert
/// and the recency order alone is under test — the grouping has its own tests
/// (see [`list_sessions_page_lists_open_sessions_before_closed_ones`]).
///
/// [`list_sessions_page_lists_open_sessions_before_closed_ones`]: super::list_sessions_page_lists_open_sessions_before_closed_ones
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
    // seeded transcript growth below; each is closed again before the walk so
    // every row goes through the closed (recency-ordered) stream.
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
    for id in ["sess-old", "sess-new", "sess-quiet"] {
        ix.close_session(&SessionId::from(id)).await.unwrap();
    }

    // Page through two at a time; concatenating the pages yields the same order
    // the all-at-once method asserts.
    let first = ix.list_sessions_page(None, 2).await.unwrap();
    assert_eq!(listed_ids(&first), vec!["sess-new", "sess-quiet"]);
    assert!(first.next.is_some(), "a full page yields a cursor");

    let second = ix.list_sessions_page(first.next, 2).await.unwrap();
    assert_eq!(listed_ids(&second), vec!["sess-old"]);
    assert!(
        second.next.is_none(),
        "a short last page yields no further cursor"
    );
}
