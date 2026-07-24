//! Permission-request recording and settlement.

use delta_model::PermissionStatus;

use super::super::SqliteStore;
use super::new_session;

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
