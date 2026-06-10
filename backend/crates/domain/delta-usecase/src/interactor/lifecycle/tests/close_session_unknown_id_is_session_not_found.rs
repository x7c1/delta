use delta_model::SessionId;

use crate::interactor::testing::*;

/// Closing an *unknown* session id is rejected with `SessionNotFound` (the
/// variant the API layer maps to 404), symmetric with `open_session`. This keeps
/// "already closed" distinguishable from "no such session" so a stale id does not
/// silently succeed, and no pane is killed.
#[tokio::test]
async fn close_session_unknown_id_is_session_not_found() {
    use crate::error::Error;

    let ix = interactor();
    let err = ix
        .close_session(&SessionId::from("ghost"))
        .await
        .expect_err("closing a non-existent session must be rejected");
    assert!(
        matches!(err, Error::SessionNotFound(id) if id == "ghost"),
        "the missing id is surfaced as SessionNotFound"
    );
    assert!(
        ix.tmux_fake().killed.lock().unwrap().is_empty(),
        "a rejected close must not kill a pane"
    );
}
