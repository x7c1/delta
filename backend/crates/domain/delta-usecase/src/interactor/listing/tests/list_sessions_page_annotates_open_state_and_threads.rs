use crate::interactor::testing::*;

/// Page rows carry the same `open` and `main_thread_id` enrichment the
/// all-at-once method does: a bound session pages as `open: true` with its
/// trunk thread id; the inline `last_activity_at` is preserved.
#[tokio::test]
async fn list_sessions_page_annotates_open_state_and_threads() {
    let ix = interactor();

    ix.new_session().await.unwrap();
    let id = ix.pending_session_ids().await.remove(0);
    ix.on_user_prompt_submit(submit_in(
        id.as_str(),
        "/work/delta-1/t.jsonl",
        "/work/delta-1",
        "hi",
    ))
    .await
    .unwrap();
    assert!(ix.pane_for_session(&id).await.is_some(), "bound = open");

    let page = ix.list_sessions_page(None, 30).await.unwrap();
    let listing = page
        .listings
        .iter()
        .find(|l| l.session.id == id)
        .expect("the session is paged");
    assert!(listing.open, "a bound session pages as open");
    assert!(
        listing.main_thread_id.value() > 0,
        "the page carries the trunk thread id"
    );
    assert!(
        listing.last_activity_at.is_none(),
        "no ingested messages means no inline last activity"
    );
}
