//! The same settle as the explicit close, reached with nobody pressing
//! anything: the liveness tick finds the pane gone while a permission dialog is
//! on screen.
//!
//! This is the worse half of the bug the shared settle fixes. The agent is
//! killed mid-dialog, the sweep closes the session a tick later, and — without
//! the settle — the browser cleared its notice on `session_closed` and then
//! re-raised the dialog from the refetch that same event triggers, over a
//! session that is now closed. Nobody asked for any of it.

use std::time::Instant;

use delta_model::{PermissionStatus, SessionId};

use crate::error::Error;
use crate::interactor::testing::*;
use crate::interactor::PermissionDecision;
use crate::ports::SessionEvent;

const TRANSCRIPT: &str = "/work/delta-1/t.jsonl";

#[tokio::test]
async fn a_vanished_pane_settles_the_dialog_it_stranded_before_closing() {
    let ix = interactor();

    // A real spawn, bound by its first hook: the pane `delta-1` exists in tmux
    // and the session is open on it.
    ix.new_session().await.unwrap();
    let session: SessionId = ix.pending_session_ids().await.remove(0);
    ix.on_user_prompt_submit(submit_in(
        session.as_str(),
        TRANSCRIPT,
        "/work/delta-1",
        "hi",
    ))
    .await
    .unwrap();

    // A permission dialog is up, with its hook parked on the answer.
    let wait = ix
        .on_permission_request(
            &session,
            "Bash",
            r#"{"command":"rm -rf /tmp/x"}"#,
            TRANSCRIPT,
        )
        .await
        .unwrap();
    assert!(
        ix.live_state_for(&session)
            .await
            .pending_permission()
            .is_some(),
        "the dialog is up when the pane disappears"
    );

    // The tmux session is killed from outside Delta. No hook arrives.
    ix.tmux_fake().vanish_session("delta-1");
    let events = ix
        .reap_stale_spawns(Instant::now(), TICK_BOUND)
        .await
        .unwrap();

    let resolved = events
        .iter()
        .position(|event| {
            matches!(
                event,
                SessionEvent::PermissionResolved { session_id, request_id }
                    if *session_id == session && *request_id == wait.request_id
            )
        })
        .unwrap_or_else(|| panic!("the stranded dialog is settled: {events:?}"));
    let closed = events
        .iter()
        .position(|event| {
            matches!(event, SessionEvent::SessionClosed { session_id } if *session_id == session)
        })
        .expect("the session is announced closed");
    assert!(
        resolved < closed,
        "the settle reaches the browser before the close it belongs to: {events:?}"
    );

    assert!(
        ix.live_state_for(&session)
            .await
            .pending_permissions
            .is_empty(),
        "nothing is left for the refetch that `session_closed` triggers to re-raise"
    );
    let rows = ix.store().inner.lock().unwrap().permissions.clone();
    let row = rows
        .iter()
        .find(|row| row.id == wait.request_id)
        .expect("the request was recorded");
    assert_eq!(
        row.status,
        PermissionStatus::Denied,
        "the row is settled, not left pending: {row:?}"
    );
    assert_eq!(
        row.decision_reason.as_deref(),
        Some("the session was closed before this request could be answered"),
        "the row records that the close is what settled it"
    );

    let result = ix
        .decide_permission(wait.request_id, PermissionDecision::Allow)
        .await;
    assert!(
        matches!(result, Err(Error::PermissionNotPending(id)) if id == wait.request_id),
        "a decision for a stranded request is a conflict, got {result:?}"
    );
}

#[tokio::test]
async fn a_vanished_pane_with_nothing_pending_announces_nothing_extra() {
    let ix = interactor();
    ix.new_session().await.unwrap();
    let session: SessionId = ix.pending_session_ids().await.remove(0);
    ix.on_user_prompt_submit(submit_in(
        session.as_str(),
        TRANSCRIPT,
        "/work/delta-1",
        "hi",
    ))
    .await
    .unwrap();

    ix.tmux_fake().vanish_session("delta-1");
    let events = ix
        .reap_stale_spawns(Instant::now(), TICK_BOUND)
        .await
        .unwrap();

    assert!(
        !events
            .iter()
            .any(|event| matches!(event, SessionEvent::PermissionResolved { .. })),
        "nothing was pending, so nothing is settled: {events:?}"
    );
}
