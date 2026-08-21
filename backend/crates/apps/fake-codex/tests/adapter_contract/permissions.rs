//! The approval round-trip for both approval kinds: an
//! `item/commandExecution/requestApproval` and an
//! `item/fileChange/requestApproval` surface as `PermissionRequested` and are
//! answered allow or deny through the trait object the core holds.

use agent_contract::launch_request;
use delta_usecase::{AgentAdapter, AgentEvent, PermissionDecision, SendRequest};

use crate::support::{adapter_with, collect_until, turn_scenario};

/// A command-execution approval surfaces as `PermissionRequested` (its `command`
/// naming the tool); answering allow emits `PermissionResolved(Allow)`.
#[tokio::test]
async fn command_execution_permission_can_be_allowed() {
    permission_case(
        PermissionDecision::Allow,
        command_execution_approval(),
        "date",
    )
    .await;
}

/// The same command-execution path, answered deny.
#[tokio::test]
async fn command_execution_permission_can_be_denied() {
    permission_case(
        PermissionDecision::Deny,
        command_execution_approval(),
        "date",
    )
    .await;
}

/// A file-change approval surfaces as `PermissionRequested` (named by its kind,
/// as its params carry no command); answering allow emits
/// `PermissionResolved(Allow)`.
#[tokio::test]
async fn file_change_permission_can_be_allowed() {
    permission_case(
        PermissionDecision::Allow,
        file_change_approval(),
        "file_change",
    )
    .await;
}

/// The same file-change path, answered deny.
#[tokio::test]
async fn file_change_permission_can_be_denied() {
    permission_case(
        PermissionDecision::Deny,
        file_change_approval(),
        "file_change",
    )
    .await;
}

/// A scripted command-execution approval step, with the real method + params.
fn command_execution_approval() -> &'static str {
    r#"{ "type": "request_approval", "method": "item/commandExecution/requestApproval",
         "params": { "itemId": "m1", "command": "date", "cwd": "/tmp" } },"#
}

/// A scripted file-change approval step, with the real method + params.
fn file_change_approval() -> &'static str {
    r#"{ "type": "request_approval", "method": "item/fileChange/requestApproval",
         "params": { "itemId": "m1", "grantRoot": "/repo", "reason": "write access" } },"#
}

async fn permission_case(decision: PermissionDecision, extra: &str, expected_tool: &str) {
    let (adapter, _guard) = adapter_with(&turn_scenario(extra)).await;
    let handle = adapter.launch(launch_request()).await.expect("launch");
    let mut stream = adapter.events(&handle);
    adapter
        .send(
            &handle,
            SendRequest {
                text: "do it".to_owned(),
            },
        )
        .await
        .expect("send");

    // Collect up to and including the PermissionRequested, then answer it.
    let before = collect_until(&mut stream, |e| {
        matches!(e, AgentEvent::PermissionRequested { .. })
    })
    .await;
    let request_id = before
        .iter()
        .find_map(|e| match e {
            AgentEvent::PermissionRequested { request } => Some(request.request_id.clone()),
            _ => None,
        })
        .expect("a PermissionRequested to answer");
    assert!(
        before.iter().any(|e| matches!(
            e,
            AgentEvent::PermissionRequested { request } if request.tool_name == expected_tool
        )),
        "the approval carries its tool name `{expected_tool}`: {before:?}"
    );

    // Answer through `&dyn AgentAdapter` (not the concrete type) so the test
    // proves the decision seam is reachable over the trait object the core holds
    // — `resolve_permission` is a trait method, not an inherent one.
    let adapter_dyn: &dyn AgentAdapter = &adapter;
    adapter_dyn
        .resolve_permission(&handle, &request_id, decision)
        .await
        .expect("resolve_permission");

    let after = collect_until(&mut stream, |e| {
        matches!(e, AgentEvent::PermissionResolved { .. })
    })
    .await;
    assert!(
        after.iter().any(|e| matches!(
            e,
            AgentEvent::PermissionResolved { request_id: rid, decision: d }
                if *rid == request_id && *d == decision
        )),
        "expected PermissionResolved({decision:?}) for {request_id}, got {after:?}"
    );
}
