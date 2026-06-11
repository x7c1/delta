use delta_model::SessionId;

use crate::interactor::testing::*;

/// `SessionStart` carrying a session id Delta knows nothing about — no pending
/// spawn, no resuming entry — is a safe no-op for both `startup` and `resume`:
/// nothing is bound, registered, released, or torn down.
#[tokio::test]
async fn session_start_unknown_session_is_a_safe_noop() {
    for source in ["startup", "resume"] {
        let ix = interactor();
        let unknown = SessionId::from("does-not-exist");

        let events = ix
            .on_session_start(session_start(unknown.as_str(), source))
            .await
            .unwrap();

        assert!(events.is_empty(), "{source} for an unknown id emits nothing");
        assert!(
            ix.store().session(&unknown).await.unwrap().is_none(),
            "{source} for an unknown id registers nothing"
        );
        assert!(
            ix.tmux_fake().sent.lock().unwrap().is_empty(),
            "{source} for an unknown id dispatches nothing"
        );
    }
}
