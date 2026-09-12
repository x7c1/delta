use delta_model::SessionId;

use crate::error::Error;
use crate::interactor::testing::*;

/// Removing an *unknown* session id is rejected with `SessionNotFound` (the
/// variant the API layer maps to 404), symmetric with `close_session`, so a
/// stale card cannot silently report success for a session nobody has.
#[tokio::test]
async fn delete_session_unknown_id_is_session_not_found() {
    let ix = interactor();
    let err = ix
        .delete_session(&SessionId::from("ghost"))
        .await
        .expect_err("removing a non-existent session must be rejected");
    assert!(
        matches!(err, Error::SessionNotFound(id) if id == "ghost"),
        "the missing id is surfaced as SessionNotFound"
    );
}
