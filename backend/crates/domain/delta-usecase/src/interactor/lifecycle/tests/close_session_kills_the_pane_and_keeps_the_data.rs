use crate::interactor::testing::*;

/// `close_session` kills the pane (recorded by the fake) and removes it from the
/// registry, while the session data remains in the store.
#[tokio::test]
async fn close_session_kills_the_pane_and_keeps_the_data() {
    let ix = interactor();
    // Spawn and bind a session.
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
    assert!(
        ix.pane_for_session(&id).await.is_some(),
        "open before close"
    );

    ix.close_session(&id).await.unwrap();

    // The pane was killed by token, and the session is no longer open.
    assert_eq!(
        ix.tmux_fake().killed.lock().unwrap().clone(),
        vec!["delta-1".to_owned()],
    );
    assert!(ix.pane_for_session(&id).await.is_none(), "closed");
    // The data session remains.
    assert!(ix.store().session(&id).await.unwrap().is_some());
}
