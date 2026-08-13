//! Permission-request recording and settlement.

use delta_model::PermissionStatus;

use super::super::SqliteStore;
use super::{new_session, new_session_with};

#[tokio::test]
async fn permission_request_is_recorded() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, _) = store.register_session(new_session()).await.unwrap();
    // The PreToolUse row carries the correlating tool_use_id...
    let req = store
        .record_permission_request(&session.id, "Bash", r#"{"command":"ls"}"#, Some("toolu_01"))
        .await
        .unwrap();
    assert_eq!(req.tool_name, "Bash");
    assert_eq!(req.tool_use_id.as_deref(), Some("toolu_01"));
    assert!(req.id > 0);
    // ...and the PermissionRequest-owned dialog row records none (NULL, never
    // an empty-string sentinel).
    let dialog = store
        .record_permission_request(&session.id, "Bash", r#"{"command":"ls"}"#, None)
        .await
        .unwrap();
    assert_eq!(dialog.tool_use_id, None);
}

#[tokio::test]
async fn permission_request_resolves_by_tool_use_id() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, _) = store.register_session(new_session()).await.unwrap();
    let req = store
        .record_permission_request(&session.id, "Bash", r#"{"command":"ls"}"#, Some("toolu_01"))
        .await
        .unwrap();

    // A non-matching tool_use_id resolves nothing.
    assert_eq!(
        store
            .resolve_permission_by_tool_use_id(&session.id, "toolu_other", true)
            .await
            .unwrap(),
        Vec::<i64>::new(),
    );

    // The matching, still-pending request resolves and returns its id.
    assert_eq!(
        store
            .resolve_permission_by_tool_use_id(&session.id, "toolu_01", true)
            .await
            .unwrap(),
        vec![req.id],
    );

    // A second resolve is a no-op: the request is no longer pending.
    assert_eq!(
        store
            .resolve_permission_by_tool_use_id(&session.id, "toolu_01", true)
            .await
            .unwrap(),
        Vec::<i64>::new(),
    );
}

#[tokio::test]
async fn resolve_settles_the_pending_dialog_row_too() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, _) = store.register_session(new_session()).await.unwrap();
    // The PreToolUse row and the hook-owned dialog row for the same call.
    let pre = store
        .record_permission_request(
            &session.id,
            "Bash",
            r#"{"command":"rm x"}"#,
            Some("toolu_01"),
        )
        .await
        .unwrap();
    let dialog = store
        .record_permission_request(&session.id, "Bash", r#"{"command":"rm x"}"#, None)
        .await
        .unwrap();

    // The tool_result settles both: the matching PreToolUse row and the
    // session's pending dialog row (the dialog blocked the session, so this
    // result is the one it gated).
    let mut resolved = store
        .resolve_permission_by_tool_use_id(&session.id, "toolu_01", false)
        .await
        .unwrap();
    resolved.sort_unstable();
    assert_eq!(resolved, vec![pre.id, dialog.id]);
}

#[tokio::test]
async fn decide_permission_request_decides_only_a_pending_row() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, _) = store.register_session(new_session()).await.unwrap();
    let req = store
        .record_permission_request(&session.id, "Bash", r#"{"command":"ls"}"#, None)
        .await
        .unwrap();

    // The first decision lands: status + decided_at recorded, row returned.
    let decided = store
        .decide_permission_request(req.id, true)
        .await
        .unwrap()
        .expect("the pending row is decided");
    assert_eq!(decided.status, PermissionStatus::Allowed);
    assert!(decided.decided_at.is_some());
    assert_eq!(decided.session_id, session.id);

    // A second decision (or one for an unknown id) decides nothing.
    assert!(store
        .decide_permission_request(req.id, false)
        .await
        .unwrap()
        .is_none());
    assert!(store
        .decide_permission_request(9999, true)
        .await
        .unwrap()
        .is_none());
}

/// The disposition for requests nobody can answer any more: a session whose
/// agent process died. Every still-`pending` row of *that* session is denied and
/// carries the reason, while an already-decided row keeps the answer it was given
/// and another session's rows are untouched.
#[tokio::test]
async fn denying_a_sessions_pending_requests_records_the_reason_and_spares_the_rest() {
    const REASON: &str = "the agent session ended before this request could be answered";
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, _) = store.register_session(new_session()).await.unwrap();
    let (other, _) = store
        .register_session(new_session_with("sess-2"))
        .await
        .unwrap();

    let pending = store
        .record_permission_request(&session.id, "Bash", r#"{"command":"cat a"}"#, None)
        .await
        .unwrap();
    let already_decided = store
        .record_permission_request(&session.id, "Bash", r#"{"command":"cat b"}"#, None)
        .await
        .unwrap();
    store
        .decide_permission_request(already_decided.id, true)
        .await
        .unwrap()
        .expect("the row was pending");
    let elsewhere = store
        .record_permission_request(&other.id, "Bash", r#"{"command":"cat c"}"#, None)
        .await
        .unwrap();

    let denied = store
        .deny_pending_permission_requests(&session.id, REASON)
        .await
        .unwrap();
    assert_eq!(
        denied,
        vec![pending.id],
        "only this session's still-pending row transitioned"
    );

    // The denied row records what happened, so the trail is not just "denied";
    // read straight off the table (nothing reads permission rows back in
    // production, so there is no store method to go through).
    assert!(
        store
            .decide_permission_request(pending.id, true)
            .await
            .unwrap()
            .is_none(),
        "the denied row is no longer pending"
    );
    async fn row(store: &SqliteStore, id: i64) -> (String, Option<String>, Option<String>) {
        let conn = store.conn.lock().await;
        conn.query_row(
            "SELECT status, decision_reason, decided_at FROM permission_request WHERE id = ?1",
            rusqlite::params![id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .unwrap()
    }
    let (status, reason, decided_at) = row(&store, pending.id).await;
    assert_eq!(status, PermissionStatus::Denied.as_str());
    assert_eq!(reason.as_deref(), Some(REASON));
    assert!(decided_at.is_some());
    // The already-answered row kept its answer; the other session's row is
    // still pending and answerable.
    let (status, reason, _) = row(&store, already_decided.id).await;
    assert_eq!(status, PermissionStatus::Allowed.as_str());
    assert_eq!(reason, None);
    assert!(
        store
            .decide_permission_request(elsewhere.id, true)
            .await
            .unwrap()
            .is_some(),
        "another session's request is untouched and still answerable"
    );

    // A second sweep finds nothing left to deny.
    assert_eq!(
        store
            .deny_pending_permission_requests(&session.id, REASON)
            .await
            .unwrap(),
        Vec::<i64>::new(),
    );
}
