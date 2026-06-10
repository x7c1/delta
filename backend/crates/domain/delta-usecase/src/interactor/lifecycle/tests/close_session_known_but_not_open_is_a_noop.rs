use delta_model::SessionId;

use crate::interactor::testing::*;

/// Closing a *known* session that is not open is a no-op: no pane is killed and
/// no error is raised, so a stale close from the browser is harmless.
#[tokio::test]
async fn close_session_known_but_not_open_is_a_noop() {
    let ix = interactor();
    // Register a known-but-closed session (an external claude): it has a store
    // row but no live pane.
    ix.on_user_prompt_submit(submit_in(
        "sess-closed",
        "/elsewhere/t.jsonl",
        "/elsewhere",
        "seed",
    ))
    .await
    .unwrap();
    let id = SessionId::from("sess-closed");
    assert!(ix.pane_for_session(&id).await.is_none(), "starts closed");

    ix.close_session(&id)
        .await
        .expect("closing a known non-open session is a no-op, not an error");
    assert!(
        ix.tmux_fake().killed.lock().unwrap().is_empty(),
        "no pane is killed when nothing was open"
    );
}
