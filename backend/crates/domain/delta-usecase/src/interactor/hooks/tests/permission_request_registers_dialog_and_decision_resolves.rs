use delta_model::{PermissionStatus, SessionId};

use crate::error::Error;
use crate::interactor::PermissionDecision;
use crate::interactor::testing::*;
use crate::ports::SessionEvent;

#[tokio::test]
async fn permission_request_records_its_own_row_and_notifies() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    let wait = ix
        .on_permission_request(&session, "Bash", r#"{"command":"rm -i x"}"#, SEED_TRANSCRIPT_PATH)
        .await
        .unwrap();

    // The hook owns its row: created here, with no tool_use_id (the payload
    // carries none) — independent of anything PreToolUse recorded.
    let recorded = {
        let g = ix.store().inner.lock().unwrap();
        g.permissions
            .iter()
            .find(|r| r.id == wait.request_id)
            .expect("the dialog row is recorded")
            .clone()
    };
    assert_eq!(recorded.tool_use_id, None);
    assert_eq!(recorded.status, PermissionStatus::Pending);

    // The notice event carries everything the browser shows next to its
    // Allow/Deny buttons, and references the hook-owned row.
    match wait.events.as_slice() {
        [SessionEvent::PermissionRequested {
            request_id,
            tool_name,
            tool_input_json,
            ..
        }] => {
            assert_eq!(*request_id, wait.request_id);
            assert_eq!(tool_name, "Bash");
            assert_eq!(tool_input_json, r#"{"command":"rm -i x"}"#);
        }
        other => panic!("expected a single PermissionRequested, got {other:?}"),
    }
}

#[tokio::test]
async fn a_browser_decision_wakes_the_hook_and_resolves_the_row() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    let wait = ix
        .on_permission_request(&session, "Bash", r#"{"command":"ls"}"#, SEED_TRANSCRIPT_PATH)
        .await
        .unwrap();

    let events = ix
        .decide_permission(wait.request_id, PermissionDecision::Allow)
        .await
        .unwrap();

    // The blocked hook receives the decision...
    assert_eq!(wait.decision.await.unwrap(), PermissionDecision::Allow);
    // ...the row records it...
    {
        let g = ix.store().inner.lock().unwrap();
        let row = g
            .permissions
            .iter()
            .find(|r| r.id == wait.request_id)
            .unwrap();
        assert_eq!(row.status, PermissionStatus::Allowed);
        assert!(row.decided_at.is_some());
    }
    // ...and the browser is told the notice is settled.
    assert_eq!(
        events,
        vec![SessionEvent::PermissionResolved {
            session_id: session,
            request_id: wait.request_id,
        }],
    );
}

#[tokio::test]
async fn a_deny_decision_marks_the_row_denied() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    let wait = ix
        .on_permission_request(&session, "Bash", r#"{"command":"rm -rf /"}"#, SEED_TRANSCRIPT_PATH)
        .await
        .unwrap();
    ix.decide_permission(wait.request_id, PermissionDecision::Deny)
        .await
        .unwrap();

    assert_eq!(wait.decision.await.unwrap(), PermissionDecision::Deny);
    let g = ix.store().inner.lock().unwrap();
    let row = g
        .permissions
        .iter()
        .find(|r| r.id == wait.request_id)
        .unwrap();
    assert_eq!(row.status, PermissionStatus::Denied);
}

#[tokio::test]
async fn deciding_after_the_wait_was_abandoned_is_a_conflict() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    let wait = ix
        .on_permission_request(&session, "Bash", r#"{"command":"ls"}"#, SEED_TRANSCRIPT_PATH)
        .await
        .unwrap();
    // The transport's deadline fired: the waiter is abandoned and the hook
    // passes through to the TUI prompt. The row stays pending for the
    // tool_result fallback.
    ix.abandon_permission_decision(wait.request_id).await;

    let err = ix
        .decide_permission(wait.request_id, PermissionDecision::Allow)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::PermissionNotPending(id) if id == wait.request_id));
    let g = ix.store().inner.lock().unwrap();
    let row = g
        .permissions
        .iter()
        .find(|r| r.id == wait.request_id)
        .unwrap();
    assert_eq!(
        row.status,
        PermissionStatus::Pending,
        "a rejected decision leaves the row to the tool_result fallback"
    );
}

#[tokio::test]
async fn deciding_an_unknown_request_is_a_conflict() {
    let ix = interactor();
    let err = ix
        .decide_permission(999, PermissionDecision::Allow)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::PermissionNotPending(999)));
}

#[tokio::test]
async fn a_second_decision_for_the_same_request_is_a_conflict() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    let wait = ix
        .on_permission_request(&session, "Bash", r#"{"command":"ls"}"#, SEED_TRANSCRIPT_PATH)
        .await
        .unwrap();
    ix.decide_permission(wait.request_id, PermissionDecision::Allow)
        .await
        .unwrap();

    let err = ix
        .decide_permission(wait.request_id, PermissionDecision::Deny)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::PermissionNotPending(_)));
    let g = ix.store().inner.lock().unwrap();
    let row = g
        .permissions
        .iter()
        .find(|r| r.id == wait.request_id)
        .unwrap();
    assert_eq!(
        row.status,
        PermissionStatus::Allowed,
        "the first decision stands"
    );
}
