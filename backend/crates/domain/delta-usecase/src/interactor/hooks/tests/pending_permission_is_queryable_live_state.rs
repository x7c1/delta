//! The pending permission dialog as queryable live state.
//!
//! The `PermissionRequested` broadcast is lost for a client whose socket was
//! down when it fired, so the dialog is mirrored into the session's runtime
//! state and reported by `live_state_for` (the sends envelope). These tests
//! pin its lifecycle: present from the hook until the request resolves (a
//! browser decision or the correlated tool_result) or the turn ends, and —
//! unlike the decision waiter — surviving the decision-wait timeout, because
//! the TUI prompt is still up then.

use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::interactor::PermissionDecision;
use crate::ports::StopHook;

#[tokio::test]
async fn the_dialog_is_reported_from_the_hook_and_survives_an_abandoned_wait() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    assert_eq!(
        ix.live_state_for(&session).await.pending_permission,
        None,
        "nothing is pending before the hook fires"
    );

    let wait = ix
        .on_permission_request(
            &session,
            "Bash",
            r#"{"command":"rm -i x"}"#,
            SEED_TRANSCRIPT_PATH,
        )
        .await
        .unwrap();

    let pending = ix
        .live_state_for(&session)
        .await
        .pending_permission
        .expect("the dialog is queryable while it awaits an answer");
    assert_eq!(pending.request_id, wait.request_id);
    assert_eq!(pending.tool_name, "Bash");
    assert_eq!(pending.tool_input_json, r#"{"command":"rm -i x"}"#);

    // The transport's decision deadline fires: the waiter is abandoned, but
    // the TUI prompt now owns the question — still genuinely pending, so the
    // queryable state must keep reporting it.
    ix.abandon_permission_decision(wait.request_id).await;
    assert!(
        ix.live_state_for(&session)
            .await
            .pending_permission
            .is_some(),
        "an abandoned wait leaves the dialog pending (the TUI prompt is up)"
    );
}

#[tokio::test]
async fn a_browser_decision_clears_the_queryable_dialog() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    let wait = ix
        .on_permission_request(
            &session,
            "Bash",
            r#"{"command":"ls"}"#,
            SEED_TRANSCRIPT_PATH,
        )
        .await
        .unwrap();
    ix.decide_permission(wait.request_id, PermissionDecision::Allow)
        .await
        .unwrap();

    assert_eq!(
        ix.live_state_for(&session).await.pending_permission,
        None,
        "a decided dialog is no longer reported"
    );
}

#[tokio::test]
async fn the_turn_ending_clears_the_queryable_dialog() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    ix.on_permission_request(
        &session,
        "Bash",
        r#"{"command":"ls"}"#,
        SEED_TRANSCRIPT_PATH,
    )
    .await
    .unwrap();
    // The turn ends (Stop hook): the dialog blocked that turn, so however the
    // question went away, it cannot outlive the turn — exactly the browser
    // notice's lifecycle.
    ix.on_stop(StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();

    assert_eq!(
        ix.live_state_for(&session).await.pending_permission,
        None,
        "a dialog never outlives its turn"
    );
}
