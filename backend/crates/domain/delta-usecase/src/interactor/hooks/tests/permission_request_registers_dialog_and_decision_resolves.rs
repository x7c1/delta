use delta_model::{PermissionStatus, SessionId};

use crate::error::Error;
use crate::interactor::testing::*;
use crate::interactor::PermissionDecision;
use crate::ports::SessionEvent;

#[tokio::test]
async fn permission_request_records_its_own_row_and_notifies() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    let wait = ix
        .on_permission_request(
            &session,
            "Bash",
            r#"{"command":"rm -i x"}"#,
            SEED_TRANSCRIPT_PATH,
        )
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
        .on_permission_request(
            &session,
            "Bash",
            r#"{"command":"ls"}"#,
            SEED_TRANSCRIPT_PATH,
        )
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
        .on_permission_request(
            &session,
            "Bash",
            r#"{"command":"rm -rf /"}"#,
            SEED_TRANSCRIPT_PATH,
        )
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
        .on_permission_request(
            &session,
            "Bash",
            r#"{"command":"ls"}"#,
            SEED_TRANSCRIPT_PATH,
        )
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

/// A session-scoped allow is refused for a provider that does not declare the
/// capability — and refused *without consequences*.
///
/// The pane-backed (Claude) path answers through the permission hook, whose
/// response carries a per-request `behavior` and has no session-scoped form.
/// Delta could quietly send `allow` instead; it deliberately does not, because a
/// user who asked to stop being prompted would go on being prompted with nothing
/// on screen explaining why.
///
/// So the refusal has to be inert: the hook is left blocked (it never sees a
/// decision it cannot express), the row stays `pending`, and — the part that is
/// easy to get wrong — the routing claim taken on the way in is handed back, so
/// the very same request is still answerable. Without that last piece the user's
/// mis-aimed click would poison the dialog: the follow-up Allow would meet a
/// `409` and the live prompt would hang.
#[tokio::test]
async fn a_session_scoped_allow_is_refused_and_leaves_the_request_answerable() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    let mut wait = ix
        .on_permission_request(
            &session,
            "Bash",
            r#"{"command":"ls"}"#,
            SEED_TRANSCRIPT_PATH,
        )
        .await
        .unwrap();

    let err = ix
        .decide_permission(wait.request_id, PermissionDecision::AllowForSession)
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::PermissionDecisionUnsupported(id) if id == wait.request_id),
        "the decision value is refused, not the request state, got {err:?}"
    );

    // The blocked hook was not woken: nothing was sent down the waiter, so the
    // agent is still waiting for an answer it can act on.
    assert!(
        wait.decision.try_recv().is_err(),
        "the hook must not receive a decision it cannot express"
    );
    // And the row is untouched.
    {
        let g = ix.store().inner.lock().unwrap();
        let row = g
            .permissions
            .iter()
            .find(|r| r.id == wait.request_id)
            .unwrap();
        assert_eq!(
            row.status,
            PermissionStatus::Pending,
            "a refused decision writes nothing"
        );
        assert!(row.decided_at.is_none());
    }

    // The same request still answers to a decision this provider does have.
    ix.decide_permission(wait.request_id, PermissionDecision::Allow)
        .await
        .unwrap();
    assert_eq!(wait.decision.await.unwrap(), PermissionDecision::Allow);
}
