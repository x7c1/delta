use crate::interactor::testing::*;

/// Equal-recency sessions page in the same deterministic id-ascending order as
/// `list_sessions_breaks_recency_ties_deterministically`, with the cursor
/// stepping cleanly across the tie group.
#[tokio::test]
async fn list_sessions_page_breaks_recency_ties_deterministically() {
    let ix = interactor();

    // Two message-less sessions share the same created_at fallback, so only the
    // ascending id tiebreaker orders them.
    ix.on_user_prompt_submit(submit_for("sess-b", "/tmp/b.jsonl", "seed"))
        .await
        .unwrap();
    ix.on_user_prompt_submit(submit_for("sess-a", "/tmp/a.jsonl", "seed"))
        .await
        .unwrap();

    let first = ix.list_sessions_page(None, 1).await.unwrap();
    assert_eq!(first.listings[0].session.id.as_str(), "sess-a");

    let second = ix.list_sessions_page(first.next, 1).await.unwrap();
    assert_eq!(second.listings[0].session.id.as_str(), "sess-b");
}
