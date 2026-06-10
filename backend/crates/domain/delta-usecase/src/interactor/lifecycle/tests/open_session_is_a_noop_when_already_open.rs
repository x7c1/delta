use delta_model::SessionId;

use crate::interactor::testing::*;

/// Opening an already-open session does not spawn a second pane (double-open
/// guard): it routes to the existing one.
#[tokio::test]
async fn open_session_is_a_noop_when_already_open() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit_in(
        "sess-R",
        "/elsewhere/t.jsonl",
        "/elsewhere",
        "seed",
    ))
    .await
    .unwrap();
    let id = SessionId::from("sess-R");

    ix.open_session(&id).await.unwrap();
    let first_pane = ix.pane_for_session(&id).await.unwrap();
    let created_after_first = ix.tmux_fake().created.lock().unwrap().len();

    // A second open is a no-op: same pane, no new spawn.
    ix.open_session(&id).await.unwrap();
    assert_eq!(ix.pane_for_session(&id).await.unwrap(), first_pane);
    assert_eq!(
        ix.tmux_fake().created.lock().unwrap().len(),
        created_after_first,
        "no second pane spawned for an already-open session"
    );
}
