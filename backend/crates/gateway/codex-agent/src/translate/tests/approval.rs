//! Server → client requests: the approvals Delta models, and the rest.

use super::*;
use crate::wire::ServerRequest;

#[test]
fn a_command_execution_approval_becomes_a_permission_request() {
    // The real `item/commandExecution/requestApproval` params, as captured
    // from a live server turn: the command names the tool, `itemId` is the
    // tool-use id, and the full params (cwd, commandActions) ride input_json.
    let request = ServerRequest {
        id: json!("srv-1"),
        method: "item/commandExecution/requestApproval".to_owned(),
        params: json!({
            "threadId": "thr_1",
            "turnId": "turn_1",
            "itemId": "exec-9",
            "startedAtMs": 1_784_272_338_055_i64,
            "command": "/bin/zsh -lc date",
            "cwd": "/tmp",
            "commandActions": [{ "type": "unknown", "command": "date" }]
        }),
    };
    match classify_server_request(&request) {
        ServerRequestKind::Approval(req) => {
            assert_eq!(req.request_id, "srv-1");
            assert_eq!(
                req.tool_name, "/bin/zsh -lc date",
                "the command names the tool"
            );
            assert_eq!(req.tool_use_id, Some("exec-9".to_owned()));
            assert_eq!(
                req.input_json["cwd"], "/tmp",
                "the full params ride input_json"
            );
        }
        other => panic!("expected an approval, got {other:?}"),
    }
}

#[test]
fn a_command_execution_approval_without_a_command_falls_back_to_a_kind_label() {
    let request = ServerRequest {
        id: json!(42),
        method: "item/commandExecution/requestApproval".to_owned(),
        params: json!({
            "threadId": "t", "turnId": "tn", "itemId": "exec-1", "startedAtMs": 0
        }),
    };
    match classify_server_request(&request) {
        ServerRequestKind::Approval(req) => {
            // A non-string id renders canonically so it still keys a lookup.
            assert_eq!(req.request_id, "42");
            assert_eq!(req.tool_name, "command_execution");
            assert_eq!(req.tool_use_id, Some("exec-1".to_owned()));
        }
        other => panic!("expected an approval, got {other:?}"),
    }
}

#[test]
fn a_file_change_approval_becomes_a_permission_request() {
    let request = ServerRequest {
        id: json!("srv-2"),
        method: "item/fileChange/requestApproval".to_owned(),
        params: json!({
            "threadId": "thr_1",
            "turnId": "turn_1",
            "itemId": "fc-3",
            "startedAtMs": 0,
            "grantRoot": "/repo",
            "reason": "extra write access"
        }),
    };
    match classify_server_request(&request) {
        ServerRequestKind::Approval(req) => {
            assert_eq!(req.request_id, "srv-2");
            assert_eq!(
                req.tool_name, "file_change",
                "a file change has no command, so a kind label names it"
            );
            assert_eq!(req.tool_use_id, Some("fc-3".to_owned()));
            assert_eq!(req.input_json["grantRoot"], "/repo");
            assert_eq!(
                req.grant_root.as_deref(),
                Some("/repo"),
                "the broadest ask is lifted out of the params, not left buried in them"
            );
        }
        other => panic!("expected an approval, got {other:?}"),
    }
}

#[test]
fn a_permissions_approval_is_unsupported_not_a_decision() {
    // The permissions approval's response is a GrantedPermissionProfile, not
    // a binary decision Delta can produce, so v1 surfaces it as unsupported
    // (and the adapter answers it) rather than fabricating a grant.
    let request = ServerRequest {
        id: json!("srv-3"),
        method: "item/permissions/requestApproval".to_owned(),
        params: json!({ "threadId": "thr_1", "itemId": "p1", "permissions": {} }),
    };
    match classify_server_request(&request) {
        ServerRequestKind::Unsupported { method, .. } => {
            assert_eq!(method, "item/permissions/requestApproval");
        }
        other => panic!("expected unsupported, got {other:?}"),
    }
}

#[test]
fn an_unmodeled_server_request_is_unsupported() {
    // A method Delta does not model surfaces as unsupported, carrying its raw
    // params as detail so the adapter can log/annotate it.
    let request = ServerRequest {
        id: json!("srv-4"),
        method: "item/tool/requestUserInput".to_owned(),
        params: json!({ "threadId": "thr_1", "questions": [] }),
    };
    match classify_server_request(&request) {
        ServerRequestKind::Unsupported {
            method,
            detail_json,
        } => {
            assert_eq!(method, "item/tool/requestUserInput");
            assert_eq!(detail_json["threadId"], "thr_1");
        }
        other => panic!("expected unsupported, got {other:?}"),
    }
}
