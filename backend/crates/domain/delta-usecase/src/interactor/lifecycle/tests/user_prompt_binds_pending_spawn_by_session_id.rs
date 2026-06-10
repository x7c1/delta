use crate::interactor::testing::*;

/// A `UserPromptSubmit` carrying a pending spawn's Delta-minted session id binds
/// that spawn (pending → bound) and registers the session.
#[tokio::test]
async fn user_prompt_binds_pending_spawn_by_session_id() {
    let ix = interactor();
    // Cold-start spawn (no first prompt).
    ix.new_session().await.unwrap();

    // Delta pinned the conversation's session id at spawn time; read it back.
    let session_id = ix.pending_session_ids().await.remove(0);

    // The spawn is not yet open under that session id.
    assert!(ix.pane_for_session(&session_id).await.is_none());

    // A hook reporting the pinned session id binds and registers. The cwd is
    // unrelated to binding now, so it can be anything.
    ix.on_user_prompt_submit(submit_in(
        session_id.as_str(),
        "/work/delta-1/t.jsonl",
        "/work/delta-1",
        "hi",
    ))
    .await
    .unwrap();

    // Now bound: the pane is the spawn's pane, and the session row exists.
    assert_eq!(
        ix.pane_for_session(&session_id).await,
        Some("delta-1:0.0".to_owned())
    );
    assert!(ix.store().session(&session_id).await.unwrap().is_some());
}
