//! Tool-permission requests and their resolution.

use super::*;

/// The Claude path (and every non-file-change request): with no detail to
/// carry, the frame is byte-for-byte the one it has always been — the
/// `file_change` key is absent, not `null`.
#[test]
fn permission_requested_and_resolved_serialize_as_tagged_events() {
    assert_eq!(
        json(&WireSessionEvent::PermissionRequested {
            session_id: "sess-1".into(),
            request_id: 7,
            tool_name: "Bash".into(),
            tool_input: "{\"command\":\"rm -i x\"}".into(),
            file_change: None,
            grant_root: None,
        }),
        serde_json::json!({
            "kind": "permission_requested",
            "session_id": "sess-1",
            "request_id": 7,
            "tool_name": "Bash",
            "tool_input": "{\"command\":\"rm -i x\"}",
        }),
    );
    assert_eq!(
        json(&WireSessionEvent::PermissionResolved {
            session_id: "sess-1".into(),
            request_id: 7,
        }),
        serde_json::json!({
            "kind": "permission_resolved",
            "session_id": "sess-1",
            "request_id": 7,
        }),
    );
}

/// A file-change request carries the detail the card renders: the affected
/// paths, how each changes, the diff, and the provider's stated reason.
#[test]
fn a_file_change_permission_carries_its_paths_kinds_and_diffs() {
    assert_eq!(
        json(&WireSessionEvent::PermissionRequested {
            session_id: "sess-1".into(),
            request_id: 7,
            tool_name: "file_change".into(),
            tool_input: "{\"itemId\":\"fc_1\"}".into(),
            file_change: Some(WireFileChangeDetail {
                changes: vec![WireFileChange {
                    path: "src/lib.rs".into(),
                    kind: Some(WireFileChangeKind::Update),
                    diff: "@@ -1 +1 @@".into(),
                }],
                reason: Some("write access".into()),
            }),
            grant_root: None,
        }),
        serde_json::json!({
            "kind": "permission_requested",
            "session_id": "sess-1",
            "request_id": 7,
            "tool_name": "file_change",
            "tool_input": "{\"itemId\":\"fc_1\"}",
            "file_change": {
                "changes": [
                    { "path": "src/lib.rs", "kind": "update", "diff": "@@ -1 +1 @@" },
                ],
                "reason": "write access",
            },
        }),
    );
}

/// A `grant_root` rides the frame on its own, with no `file_change` beside
/// it — the shape an approval takes when its change set could not be
/// correlated. It must still cross: it is the broadest thing the dialog
/// grants (writes anywhere under that root for the rest of the session), so
/// a frame that dropped it would understate the request precisely where the
/// client has nothing else to show.
#[test]
fn a_grant_root_crosses_even_with_no_change_set_beside_it() {
    assert_eq!(
        json(&WireSessionEvent::PermissionRequested {
            session_id: "sess-1".into(),
            request_id: 8,
            tool_name: "file_change".into(),
            tool_input: "{\"itemId\":\"fc_2\"}".into(),
            file_change: None,
            grant_root: Some("/repo".into()),
        }),
        serde_json::json!({
            "kind": "permission_requested",
            "session_id": "sess-1",
            "request_id": 8,
            "tool_name": "file_change",
            "tool_input": "{\"itemId\":\"fc_2\"}",
            "grant_root": "/repo",
        }),
    );
}
