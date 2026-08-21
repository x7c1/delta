//! Server → client requests Delta does not model: each must surface as
//! `UnsupportedInteraction` without the turn hanging behind it.

use agent_contract::launch_request;
use delta_usecase::{AgentAdapter, AgentEvent, SendRequest};

use crate::support::{adapter_with, collect_until, is_turn_completed, turn_scenario};

/// `no_server_request_silently_hangs` (permissions approval): the permissions
/// approval is a real `*/requestApproval` method, but its response is a
/// permission profile Delta cannot synthesise — so it must NOT be treated as an
/// approval. It surfaces as `UnsupportedInteraction` and the turn still
/// completes (the adapter answered it rather than blocking on it).
#[tokio::test]
async fn permissions_approval_is_unsupported_and_never_hangs() {
    no_server_request_silently_hangs_for(
        r#"{ "type": "request_approval", "method": "item/permissions/requestApproval",
             "params": { "itemId": "m1", "cwd": "/tmp", "permissions": {} } },"#,
        "item/permissions/requestApproval",
    )
    .await;
}

/// `no_server_request_silently_hangs` (unknown method): a server → client request
/// Delta does not model at all surfaces as `UnsupportedInteraction` without the
/// turn hanging.
#[tokio::test]
async fn unknown_server_request_is_unsupported_and_never_hangs() {
    no_server_request_silently_hangs_for(
        r#"{ "type": "request_approval", "method": "item/tool/requestUserInput", "params": { "questions": [] } },"#,
        "item/tool/requestUserInput",
    )
    .await;
}

/// Drive a turn that emits a server → client request the adapter does not model,
/// asserting it surfaces as `UnsupportedInteraction` for `method` and the turn
/// still completes (the adapter answered the request rather than blocking on it).
async fn no_server_request_silently_hangs_for(extra: &str, method: &str) {
    let (adapter, _guard) = adapter_with(&turn_scenario(extra)).await;
    let handle = adapter.launch(launch_request()).await.expect("launch");
    let mut stream = adapter.events(&handle);
    adapter
        .send(
            &handle,
            SendRequest {
                text: "trigger the unmodeled request".to_owned(),
            },
        )
        .await
        .expect("send");
    // If the adapter blocked on the unmodeled request, the turn would never
    // complete and this collect would time out (a failure).
    let events = collect_until(&mut stream, is_turn_completed).await;
    assert!(
        events.iter().any(|e| matches!(
            e,
            AgentEvent::UnsupportedInteraction { method: m, .. } if m == method
        )),
        "`{method}` must surface as UnsupportedInteraction, got {events:?}"
    );
    assert!(
        events.iter().any(is_turn_completed),
        "the turn must still complete (no silent hang), got {events:?}"
    );
}
