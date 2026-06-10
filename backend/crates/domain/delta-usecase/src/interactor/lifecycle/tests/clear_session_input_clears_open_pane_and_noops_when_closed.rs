use delta_model::SessionId;

use crate::interactor::testing::*;

/// `clear_session_input` wipes the open session's pane via the driver, and is a
/// no-op (no driver call, no error) when the session is not open. This pins the
/// clear-on-attach path the PTY bridge uses before a fresh attach.
#[tokio::test]
async fn clear_session_input_clears_open_pane_and_noops_when_closed() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit_in(
        "sess-C",
        "/elsewhere/t.jsonl",
        "/elsewhere",
        "seed",
    ))
    .await
    .unwrap();
    let id = SessionId::from("sess-C");

    // Closed: clearing is a no-op that records no driver call.
    assert!(ix.pane_for_session(&id).await.is_none(), "starts closed");
    ix.clear_session_input(&id).await.unwrap();
    assert!(
        ix.tmux_fake().cleared.lock().unwrap().is_empty(),
        "a closed session has no live pane to clear"
    );

    // Open it, then clearing targets the bound pane.
    ix.open_session(&id).await.unwrap();
    let pane = ix.pane_for_session(&id).await.expect("now open");
    ix.clear_session_input(&id).await.unwrap();
    assert_eq!(
        ix.tmux_fake().cleared.lock().unwrap().clone(),
        vec![pane],
        "the open session's pane was cleared exactly once"
    );
}
