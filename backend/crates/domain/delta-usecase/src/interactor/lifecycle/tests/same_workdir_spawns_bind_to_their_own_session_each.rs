use crate::interactor::testing::*;

/// Two pending spawns that SHARE the same working directory each still bind to
/// the right session, because correlation is keyed by the Delta-minted session
/// id (pinned via `claude --session-id`), not by the cwd. This is the regression
/// guard for a future where the user picks a real project directory as the
/// session cwd: two spawns may then share a cwd without mis-correlating.
#[tokio::test]
async fn same_workdir_spawns_bind_to_their_own_session_each() {
    let ix = interactor();
    ix.new_session().await.unwrap(); // delta-1
    ix.new_session().await.unwrap(); // delta-2

    // Read back the two pinned session ids, in spawn order.
    let ids = ix.pending_session_ids().await;
    assert_eq!(ids.len(), 2, "two spawns are pending");
    let (id1, id2) = (ids[0].clone(), ids[1].clone());
    assert_ne!(id1, id2, "each spawn mints a distinct session id");

    // Fire the hooks in the OPPOSITE order to the spawn order, and crucially
    // with the SAME shared cwd for both — so only the session id can resolve
    // which spawn each binds to.
    const SHARED_CWD: &str = "/work/project";
    ix.on_user_prompt_submit(submit_in(
        id2.as_str(),
        "/work/project/t2.jsonl",
        SHARED_CWD,
        "hi",
    ))
    .await
    .unwrap();
    ix.on_user_prompt_submit(submit_in(
        id1.as_str(),
        "/work/project/t1.jsonl",
        SHARED_CWD,
        "hi",
    ))
    .await
    .unwrap();

    // Each session bound to its own spawn's pane despite the shared cwd.
    assert_eq!(
        ix.pane_for_session(&id1).await,
        Some("delta-1:0.0".to_owned()),
    );
    assert_eq!(
        ix.pane_for_session(&id2).await,
        Some("delta-2:0.0".to_owned()),
    );
}
