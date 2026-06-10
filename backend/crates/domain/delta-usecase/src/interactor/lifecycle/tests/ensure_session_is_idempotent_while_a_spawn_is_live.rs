use crate::interactor::testing::*;
use crate::ports::SessionLifecycle;

#[tokio::test]
async fn ensure_session_is_idempotent_while_a_spawn_is_live() {
    let ix = interactor();

    // First call spawns a session. It stays pending (no hook has bound it yet).
    ix.ensure_session().await.unwrap();
    // Second call finds a live (pending) spawn: reuse, no second spawn or write.
    let status = ix.ensure_session().await.unwrap();

    assert_eq!(status, SessionLifecycle::Ready);
    assert_eq!(
        ix.tmux_fake().created.lock().unwrap().len(),
        1,
        "a live spawn must not be re-spawned"
    );
    assert_eq!(
        ix.workspace_fake().written.lock().unwrap().len(),
        1,
        "settings must not be rewritten when a spawn is already live"
    );
}
