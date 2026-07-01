use delta_model::SessionId;

use crate::interactor::testing::*;

/// `list_sessions_page` lists every registered session, each annotated with its
/// live (open) state and `main` thread id; `threads_for` scopes the thread tree
/// to a single session by id. With no messages, both sessions share the same
/// recency fallback (`created_at`), so the `id` tiebreaker decides their order;
/// recency ordering proper is covered by
/// [`list_sessions_page_reproduces_recency_order_across_pages`].
///
/// [`list_sessions_page_reproduces_recency_order_across_pages`]: super::list_sessions_page_reproduces_recency_order_across_pages
#[tokio::test]
async fn list_sessions_page_annotates_each_with_open_state_and_threads_route_by_id() {
    let ix = interactor();

    // No session yet: the page is empty.
    assert!(ix
        .list_sessions_page(None, 30)
        .await
        .unwrap()
        .listings
        .is_empty());

    // Register two sessions in order. Their hooks arrive in a cwd with no
    // matching pending spawn, so they register as external, closed data sessions
    // (no live pane).
    ix.on_user_prompt_submit(submit_for("sess-1", "/tmp/s1.jsonl", "seed"))
        .await
        .unwrap();
    ix.on_user_prompt_submit(submit_for("sess-2", "/tmp/s2.jsonl", "seed"))
        .await
        .unwrap();

    let listings = ix.list_sessions_page(None, 30).await.unwrap().listings;
    let ids: Vec<_> = listings
        .iter()
        .map(|l| l.session.id.as_str().to_owned())
        .collect();
    assert_eq!(
        ids,
        vec!["sess-2", "sess-1"],
        "equal recency falls back to the deterministic id tiebreaker \
         (descending: time-ordered ids put the newest first)"
    );
    assert!(
        listings.iter().all(|l| !l.open),
        "externally-registered sessions are closed (no live pane)"
    );
    assert!(
        listings.iter().all(|l| l.main_thread_id.value() > 0),
        "every listing carries its main thread id"
    );
    assert!(
        listings.iter().all(|l| l.last_activity_at.is_none()),
        "sessions with no ingested messages have no last activity"
    );

    // `threads_for` is scoped to the named session: only its own threads.
    let threads = ix.threads_for(&SessionId::from("sess-2")).await.unwrap();
    assert!(
        !threads.is_empty() && threads.iter().all(|t| t.session_id.as_str() == "sess-2"),
        "threads belong to the requested session only"
    );

    // An unknown session id is a clean SessionNotFound, not an empty list.
    let err = ix
        .threads_for(&SessionId::from("nope"))
        .await
        .expect_err("unknown session id is rejected");
    assert!(matches!(err, crate::error::Error::SessionNotFound(_)));
}
