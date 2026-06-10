use crate::interactor::testing::*;
use crate::ports::SessionLifecycle;

/// `ensure_session` is idempotent against a *bound* session too, not only a
/// pending spawn: once a hook has bound the spawn to a session id, a further
/// `ensure_session` reuses it (`Ready`) without spawning a second pane. This
/// pins the `bound` half of `has_any_live`, which the pending-only idempotency
/// test above does not exercise.
#[tokio::test]
async fn ensure_session_is_idempotent_while_a_session_is_bound() {
    let ix = interactor();

    // Spawn, then bind it via a hook carrying the spawn's minted session id.
    ix.ensure_session().await.unwrap();
    let id = ix.pending_session_ids().await.remove(0);
    ix.on_user_prompt_submit(submit_in(
        id.as_str(),
        "/work/delta-1/t.jsonl",
        "/work/delta-1",
        "hi",
    ))
    .await
    .unwrap();
    assert!(
        ix.pane_for_session(&id).await.is_some(),
        "the spawn is now bound"
    );

    // A further ensure_session finds the bound session live: reuse, no re-spawn.
    let status = ix.ensure_session().await.unwrap();
    assert_eq!(status, SessionLifecycle::Ready);
    assert_eq!(
        ix.tmux_fake().created.lock().unwrap().len(),
        1,
        "a bound session must not be re-spawned"
    );
}
