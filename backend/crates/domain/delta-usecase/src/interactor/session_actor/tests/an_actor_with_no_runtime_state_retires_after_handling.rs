use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::ports::StopHook;

/// An actor whose runtime state ends up empty (no pane, no launch in flight,
/// idle turn, no waiters) retires instead of parking forever: a stray hook
/// for an unknown session is handled exactly as before (a safe no-op) and the
/// registry entry disappears afterwards. A later input simply spawns a fresh
/// actor, whose default state means the same thing.
#[tokio::test]
async fn an_actor_with_no_runtime_state_retires_after_handling() {
    let ix = interactor();
    let ghost = SessionId::from("sess-ghost");

    // A Stop for a session Delta has never seen: handled as the same safe
    // no-op as before (the turn machine treats Close/Stop on idle as no-ops).
    let events = ix
        .on_stop(StopHook {
            session_id: ghost.clone(),
            stop_reason: None,
        })
        .await
        .unwrap();
    assert_eq!(events.len(), 1, "the TurnCompleted broadcast is preserved");

    // The actor spawned for the ghost session retires once its mailbox is
    // empty; retirement runs right after the reply, so yield until the
    // registry entry is gone.
    for _ in 0..100 {
        if ix.sessions.ids().is_empty() {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("the empty actor should have retired and left the registry");
}
