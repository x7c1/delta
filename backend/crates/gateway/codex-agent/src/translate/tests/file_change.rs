//! File-change correlation.

use delta_usecase::{AgentFileChange, AgentFileChangeDetail, AgentFileChangeKind};

use super::*;
use crate::wire::ServerRequest;

fn update_change(path: &str, diff: &str) -> Value {
    json!({ "path": path, "kind": { "type": "update" }, "diff": diff })
}

#[test]
fn an_item_started_file_change_yields_its_id_and_changes() {
    let n = notification(
        "item/started",
        json!({
            "item": {
                "id": "fc-1",
                "type": "fileChange",
                "status": "inProgress",
                "changes": [
                    { "path": "a.rs", "kind": { "type": "add" }, "diff": "+a" },
                    { "path": "b.rs", "kind": { "type": "delete" }, "diff": "-b" },
                    update_change("c.rs", "~c"),
                ],
            },
        }),
    );

    assert_eq!(
        file_change_item(&n),
        Some((
            "fc-1".to_owned(),
            vec![
                AgentFileChange {
                    path: "a.rs".to_owned(),
                    kind: Some(AgentFileChangeKind::Add),
                    diff: "+a".to_owned(),
                },
                AgentFileChange {
                    path: "b.rs".to_owned(),
                    kind: Some(AgentFileChangeKind::Delete),
                    diff: "-b".to_owned(),
                },
                AgentFileChange {
                    path: "c.rs".to_owned(),
                    kind: Some(AgentFileChangeKind::Update),
                    diff: "~c".to_owned(),
                },
            ],
        )),
    );
}

#[test]
fn an_item_of_another_type_is_not_tracked() {
    let n = notification(
        "item/started",
        json!({ "item": { "id": "m1", "type": "agentMessage" } }),
    );

    assert_eq!(file_change_item(&n), None);
}

#[test]
fn a_patch_update_yields_the_replacement_changes() {
    let n = notification(
        METHOD_FILE_CHANGE_PATCH_UPDATED,
        json!({
            "itemId": "fc-1",
            "threadId": "thr_1",
            "turnId": "turn_1",
            "changes": [update_change("a.rs", "revised")],
        }),
    );

    assert_eq!(
        file_change_item(&n),
        Some((
            "fc-1".to_owned(),
            vec![AgentFileChange {
                path: "a.rs".to_owned(),
                kind: Some(AgentFileChangeKind::Update),
                diff: "revised".to_owned(),
            }],
        )),
    );
    assert!(
        translate_notification(&n).is_empty(),
        "the revision itself projects to no neutral event"
    );
}

#[test]
fn an_unmodeled_kind_keeps_the_path_and_diff_without_a_label() {
    // A newer server naming a kind this build does not model must not cost
    // the user the path — that is the part the answer usually turns on.
    let n = notification(
        "item/started",
        json!({
            "item": {
                "id": "fc-1",
                "type": "fileChange",
                "changes": [
                    { "path": "a.rs", "kind": { "type": "teleport" }, "diff": "?" },
                    { "kind": { "type": "add" }, "diff": "no path" },
                ],
            },
        }),
    );

    assert_eq!(
        file_change_item(&n),
        Some((
            "fc-1".to_owned(),
            vec![AgentFileChange {
                path: "a.rs".to_owned(),
                kind: None,
                diff: "?".to_owned(),
            }],
        )),
        "an unknown kind loses its label; a change with no path is dropped entirely"
    );
}

#[test]
fn a_completed_item_reports_the_id_to_forget() {
    let completed = notification(
        "item/completed",
        json!({ "item": { "id": "fc-1", "type": "fileChange", "changes": [] } }),
    );
    let started = notification(
        "item/started",
        json!({ "item": { "id": "fc-1", "type": "fileChange", "changes": [] } }),
    );

    assert_eq!(completed_item_id(&completed), Some("fc-1".to_owned()));
    assert_eq!(
        completed_item_id(&started),
        None,
        "only a completion retires an entry"
    );
}

#[test]
fn completing_a_file_change_approval_joins_its_changes_with_its_reason() {
    // The changes come from the item, the reason from the approval's own
    // params — the two halves of the same question.
    let request = ServerRequest {
        id: json!("srv-9"),
        method: METHOD_FILE_CHANGE_APPROVAL.to_owned(),
        params: json!({
            "threadId": "thr_1",
            "turnId": "turn_1",
            "itemId": "fc-1",
            "startedAtMs": 0,
            "reason": "extra write access",
        }),
    };
    let ServerRequestKind::Approval(mut approval) = classify_server_request(&request) else {
        panic!("a file-change approval is modeled");
    };
    assert_eq!(
        approval.file_change, None,
        "the pure classification cannot correlate; that is the adapter's job"
    );

    with_file_change_detail(
        &mut approval,
        vec![AgentFileChange {
            path: "a.rs".to_owned(),
            kind: Some(AgentFileChangeKind::Update),
            diff: "~a".to_owned(),
        }],
    );

    assert_eq!(
        approval.file_change,
        Some(AgentFileChangeDetail {
            changes: vec![AgentFileChange {
                path: "a.rs".to_owned(),
                kind: Some(AgentFileChangeKind::Update),
                diff: "~a".to_owned(),
            }],
            reason: Some("extra write access".to_owned()),
        }),
    );
}
