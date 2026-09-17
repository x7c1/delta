//! Pressing Close on a session with a permission dialog up must **settle** the
//! dialog, not abandon it.
//!
//! The field failure: the teardown dropped the binding and announced
//! `session_closed`, but left the request row `pending`, the hook parked on its
//! waiter, and the routing entry in place. The browser cleared its notice on
//! `session_closed` and then re-raised the dialog from the refetch that the very
//! same event triggers — now over a closed session, where Allow answers `409`
//! and the fallback text points at a terminal reading "This session is closed".
//! Dismiss was the only way out.
//!
//! The teardown now runs the same settle an adapter-backed session's death runs
//! (`settle_pending_permissions`), so a request nobody can answer is denied with
//! a reason and announced as resolved.

use delta_model::{PermissionStatus, SessionId};

use crate::error::Error;
use crate::interactor::testing::*;
use crate::interactor::PermissionDecision;
use crate::ports::SessionEvent;

/// The `decision_reason` the teardown records. Spelled out here rather than
/// imported: it is a user-visible sentence in the audit trail, so a test that
/// shared the constant would agree with any rewording of it.
const CLOSED_REASON: &str = "the session was closed before this request could be answered";

/// An open, bound session with its registration turn still in flight — the
/// state a permission dialog is raised in.
async fn open_session() -> (TestInteractor, SessionId) {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");
    ix.bind_open_session("delta-seed", &session).await;
    (ix, session)
}

/// The request ids the returned batch settled, in order.
fn resolved_ids(events: &[SessionEvent]) -> Vec<i64> {
    events
        .iter()
        .filter_map(|event| match event {
            SessionEvent::PermissionResolved { request_id, .. } => Some(*request_id),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn close_session_denies_its_pending_dialog_and_announces_the_resolution() {
    let (ix, session) = open_session().await;
    let wait = ix
        .on_permission_request(
            &session,
            "Bash",
            r#"{"command":"rm -rf /tmp/x"}"#,
            SEED_TRANSCRIPT_PATH,
        )
        .await
        .unwrap();

    let events = ix.close_session(&session).await.unwrap();

    // The whole batch is broadcast before the `SessionClosed` the API layer
    // appends, so a settle anywhere in it reaches the browser first.
    assert_eq!(
        resolved_ids(&events),
        vec![wait.request_id],
        "the stranded dialog is settled client-visibly: {events:?}"
    );

    let live = ix.live_state_for(&session).await;
    assert!(
        live.pending_permissions.is_empty(),
        "the sends envelope a client refetches on `session_closed` reports no dialog \
         to re-raise: {:?}",
        live.pending_permissions
    );

    // No row is left `pending`, and it says why it settled — a Deny with no
    // reason would be indistinguishable from a user's Deny.
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
        Some(CLOSED_REASON),
        "the row records that the close is what settled it"
    );

    // The hook parked on this request is released rather than left to its own
    // decision deadline, and it is told the tool was not permitted.
    assert_eq!(
        wait.decision.await.ok(),
        Some(PermissionDecision::Deny),
        "the blocked hook is answered as the row was recorded"
    );

    // A decision that races the close is a clean conflict, not an internal
    // error: the settle dropped the routing entry it would have travelled on.
    let result = ix
        .decide_permission(wait.request_id, PermissionDecision::Allow)
        .await;
    assert!(
        matches!(result, Err(Error::PermissionNotPending(id)) if id == wait.request_id),
        "a decision for a stranded request is a conflict, got {result:?}"
    );
}

#[tokio::test]
async fn close_session_settles_every_queued_dialog_without_raising_one() {
    // Several dialogs can be queued at once — an adapter-backed provider
    // (Codex) runs tool calls in parallel, so one turn raises N approvals,
    // while Claude's hook blocks serially and never queues more than one. The
    // queue is shared state and the teardown is one routine, so the depth is
    // driven here through the pane-backed hook rather than standing a Codex
    // session up. Clearing the queue one entry at a time through the keyed path
    // would promote each successive head and re-broadcast it — raising dialogs
    // the very settle is in the middle of clearing.
    let (ix, session) = open_session().await;
    let mut request_ids = Vec::new();
    for command in ["ls", "cat x"] {
        let wait = ix
            .on_permission_request(
                &session,
                "Bash",
                &format!(r#"{{"command":"{command}"}}"#),
                SEED_TRANSCRIPT_PATH,
            )
            .await
            .unwrap();
        request_ids.push(wait.request_id);
    }

    let events = ix.close_session(&session).await.unwrap();

    assert_eq!(
        resolved_ids(&events),
        request_ids,
        "both queued dialogs are settled: {events:?}"
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, SessionEvent::PermissionRequested { .. })),
        "no dialog is raised while clearing them all — a settle must not leave a \
         promoted head on screen: {events:?}"
    );
    let rows = ix.store().inner.lock().unwrap().permissions.clone();
    assert!(
        rows.iter()
            .all(|row| row.status == PermissionStatus::Denied),
        "no row is left pending: {rows:?}"
    );
}

#[tokio::test]
async fn close_session_settles_a_pending_question_over_the_same_event() {
    // The question card needs no settle of its own, and this is why:
    // `AskUserQuestion` is recorded on the same `permission_request` table as a
    // dialog, so the teardown's sweep denies its row and the `PermissionResolved`
    // it produces is exactly the signal that clears the card (the same one an
    // answered or cancelled question settles through). Pinned here because the
    // teardown documents it as the reason it settles nothing else.
    const QUESTION_INPUT: &str = r#"{"questions":[{"question":"Which?","header":"Pick","options":[{"label":"A","description":"first"}],"multiSelect":false}]}"#;
    let (ix, session) = open_session().await;
    ix.on_pre_tool_use(
        &session,
        "AskUserQuestion",
        QUESTION_INPUT,
        "toolu_q1",
        SEED_TRANSCRIPT_PATH,
    )
    .await
    .unwrap();
    let request_id = ix
        .live_state_for(&session)
        .await
        .pending_question
        .expect("the question is up when the session is closed")
        .request_id;

    let events = ix.close_session(&session).await.unwrap();

    assert_eq!(
        resolved_ids(&events),
        vec![request_id],
        "the stranded question is settled over the permission-resolution signal \
         its card listens on: {events:?}"
    );
    assert!(
        ix.live_state_for(&session).await.pending_question.is_none(),
        "no question is left for the refetch to re-seed"
    );
}

#[tokio::test]
async fn close_session_with_nothing_pending_announces_nothing_extra() {
    // The plainest row of the matrix: a close that strands no request invents no
    // settle for it.
    let (ix, session) = open_session().await;

    let events = ix.close_session(&session).await.unwrap();

    assert!(
        events.is_empty(),
        "a close with nothing pending emits exactly what it always did: {events:?}"
    );
}
