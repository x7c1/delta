use delta_model::SessionId;

use crate::interactor::testing::*;

/// Opening a session id that does not exist in the store is rejected with
/// `SessionNotFound` (the variant the API layer maps to 404), and no pane is
/// spawned. This is the only code path that produces `SessionNotFound`, so it
/// pins both the error and the reason its 404 mapping exists.
#[tokio::test]
async fn open_session_unknown_id_is_session_not_found() {
    use crate::error::Error;

    let ix = interactor();
    let err = ix
        .open_session(&SessionId::from("ghost"))
        .await
        .expect_err("opening a non-existent session must be rejected");
    assert!(
        matches!(err, Error::SessionNotFound(id) if id == "ghost"),
        "the missing id is surfaced as SessionNotFound"
    );
    assert!(
        ix.tmux_fake().created.lock().unwrap().is_empty(),
        "a rejected open must not spawn a pane"
    );
}
