use crate::interactor::testing::*;
use crate::ports::SessionEndHook;

/// A `SessionEnd` for an already-bound (known) session is a normal end: it
/// emits no `SpawnFailed` and does not tear the session down — close/teardown
/// semantics are left entirely to `close_session`.
#[tokio::test]
async fn session_end_for_bound_session_is_a_normal_end() {
    let ix = interactor();

    // Spawn and bind a session through the normal path.
    ix.new_session().await.unwrap();
    let id = ix.pending_session_ids().await.remove(0);
    ix.on_user_prompt_submit(submit_in(
        id.as_str(),
        "/work/delta-1/t.jsonl",
        "/work/delta-1",
        "hi",
    ))
    .await
    .unwrap();
    assert!(ix.pane_for_session(&id).await.is_some(), "bound and open");

    let events = ix
        .on_session_end(SessionEndHook {
            session_id: id.clone(),
            reason: Some("clear".into()),
        })
        .await
        .unwrap();

    // Normal end: no failure event, no teardown.
    assert!(
        events.is_empty(),
        "a bound session's end emits no SpawnFailed"
    );
    assert!(
        ix.tmux_fake().killed.lock().unwrap().is_empty(),
        "SessionEnd must not kill a bound session's pane"
    );
    assert!(
        ix.pane_for_session(&id).await.is_some(),
        "the session stays open; close/teardown is left to close_session"
    );
}
