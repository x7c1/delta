//! The allow / deny / allow-for-session matrix over both approval kinds, each
//! driven browser → server → `fake-codex` over one blocking approval.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::json;
use tower::ServiceExt;

use delta_usecase::SessionEvent;

use crate::support::{build_app, get, post_json, ScenarioGuard, TIMEOUT};

/// The Codex command-execution permission full loop, answered **allow**: the
/// approval gates the turn, the browser allows it by the Delta row id, and the
/// fake proceeds having received `accept`.
#[tokio::test(flavor = "multi_thread")]
async fn codex_command_execution_permission_full_loop_allow() {
    permission_full_loop("allow", "accept", command_execution_step(), "date").await;
}

/// The same command-execution loop, answered **deny**: the fake proceeds having
/// received `decline`, and the turn still completes.
#[tokio::test(flavor = "multi_thread")]
async fn codex_command_execution_permission_full_loop_deny() {
    permission_full_loop("deny", "decline", command_execution_step(), "date").await;
}

/// The Codex file-change permission full loop, answered **allow**: the same
/// browser → server → fake path over the real file-change approval shape.
#[tokio::test(flavor = "multi_thread")]
async fn codex_file_change_permission_full_loop_allow() {
    permission_full_loop("allow", "accept", file_change_step(), "file_change").await;
}

/// The same file-change loop, answered **deny**.
#[tokio::test(flavor = "multi_thread")]
async fn codex_file_change_permission_full_loop_deny() {
    permission_full_loop("deny", "decline", file_change_step(), "file_change").await;
}

/// The command-execution loop answered with the **session-scoped** allow: the
/// browser posts `allow_for_session` and the exact wire value `acceptForSession`
/// — not a downgraded `accept` — is what reaches the provider.
///
/// The value matters more than the status code here. Delta cannot observe the
/// grant the provider then holds (the scope lives entirely in its session), so
/// the only thing that proves the user's choice survived the whole stack is the
/// literal string the fake echoes back from the response it received.
#[tokio::test(flavor = "multi_thread")]
async fn codex_command_execution_permission_full_loop_allow_for_session() {
    permission_full_loop(
        "allow_for_session",
        "acceptForSession",
        command_execution_step(),
        "date",
    )
    .await;
}

/// The same session-scoped allow over a **file-change** approval. Both approval
/// kinds share one `{ "decision": … }` reply path in the adapter, and this is
/// what pins that the shared path really does serve both — a mapping added for
/// command execution alone would pass the test above and fail here.
#[tokio::test(flavor = "multi_thread")]
async fn codex_file_change_permission_full_loop_allow_for_session() {
    permission_full_loop(
        "allow_for_session",
        "acceptForSession",
        file_change_step(),
        "file_change",
    )
    .await;
}

/// A blocking command-execution approval step, with the real method + params;
/// `command` names the tool the browser sees.
fn command_execution_step() -> &'static str {
    r#"{ "type": "request_approval", "blocking": true,
         "method": "item/commandExecution/requestApproval",
         "params": { "itemId": "m1", "command": "date", "cwd": "/tmp" } }"#
}

/// A blocking file-change approval step, with the real method + params; it names
/// no command, so the browser sees the `file_change` kind label.
fn file_change_step() -> &'static str {
    r#"{ "type": "request_approval", "blocking": true,
         "method": "item/fileChange/requestApproval",
         "params": { "itemId": "m1", "grantRoot": "/repo", "reason": "write access" } }"#
}

/// Drive the full browser → server → `fake-codex` permission loop for one
/// approval shape.
///
/// The scenario gates its turn on a **blocking** approval: the fake emits the
/// approval and suspends until the client answers. The test waits for the
/// `PermissionRequested` broadcast (carrying the Delta `i64` row id, not the
/// provider token, and `expected_tool` as the tool name), decides via
/// `POST /api/permissions/{id}/decision`, and then asserts (a) the decision
/// settled over the broadcast (`PermissionResolved` + `TurnCompleted`) and (b)
/// the fake received exactly `expected_echo` — the Codex wire value the neutral
/// decision maps to (`accept`, `acceptForSession` or `decline`) — because the
/// fake echoes the received decision verbatim as an assistant message, which the
/// test reads back from the persisted transcript.
async fn permission_full_loop(
    decision_wire: &str,
    expected_echo: &str,
    approval_step: &str,
    expected_tool: &str,
) {
    let scenario = ScenarioGuard::write(&format!(
        r#"{{
            "thread_id": "thr_perm_loop",
            "turn": {{
                "turn_id": "turn_perm_loop",
                "emit": [
                    {{ "type": "turn_started" }},
                    {approval_step},
                    {{ "type": "turn_completed", "status": "completed" }}
                ]
            }}
        }}"#
    ));

    let (app, state) = build_app(&scenario);
    let mut events = state.subscribe();
    state
        .spawn_async_event_drain()
        .expect("the async drain is taken exactly once");

    // Create a Codex session with a first prompt.
    let (status, body) = post_json(
        &app,
        "/api/sends",
        json!({ "new_session": true, "provider": "codex", "text": "run a command" }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "the send was created: {body:?}"
    );
    let session_id = body["send"]["session_id"].as_str().unwrap().to_owned();
    let thread_id = body["send"]["thread_id"].as_i64().unwrap();

    // Wait for the approval notice. It carries the Delta row id — the decision
    // endpoint's key — not the adapter's opaque provider token.
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    let request_id = loop {
        let event = tokio::time::timeout_at(deadline, events.recv())
            .await
            .expect("timed out waiting for the permission request")
            .expect("the broadcast channel stayed open");
        if let SessionEvent::PermissionRequested {
            session_id: sid,
            request_id,
            tool_name,
            ..
        } = event
        {
            assert_eq!(sid.as_str(), session_id, "the notice names our session");
            assert_eq!(tool_name, expected_tool, "the notice carries the tool name");
            assert!(request_id > 0, "the notice carries a Delta row id");
            break request_id;
        }
    };

    // Decide by the i64 row id over the REST surface.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/permissions/{request_id}/decision"))
                .header("host", "127.0.0.1")
                .header("content-type", "application/json")
                .body(Body::from(json!({ "decision": decision_wire }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::NO_CONTENT,
        "the decision was accepted"
    );

    // The decision settles over the broadcast: the notice resolves and the turn
    // (unblocked by the answer reaching the fake) completes.
    let mut resolved = false;
    let mut turn_completed = false;
    while !(resolved && turn_completed) {
        let event = tokio::time::timeout_at(deadline, events.recv())
            .await
            .expect("timed out waiting for the decision to settle")
            .expect("the broadcast channel stayed open");
        match event {
            SessionEvent::PermissionResolved {
                request_id: rid, ..
            } => {
                assert_eq!(rid, request_id, "the settle names the same row id");
                resolved = true;
            }
            SessionEvent::TurnCompleted { .. } => turn_completed = true,
            _ => {}
        }
    }

    // The fake received the exact wire value, not one collapsed into a
    // neighbour on the way down: it echoes the decision it was handed as an
    // assistant message, which persisted through the same content path as any
    // other reply.
    let (status, body) = get(&app, &format!("/api/threads/{thread_id}/messages")).await;
    assert_eq!(status, StatusCode::OK, "messages fetched: {body:?}");
    let messages = body["messages"].as_array().unwrap();
    assert!(
        messages
            .iter()
            .any(|m| m["role"] == json!("assistant") && m["content_text"] == json!(expected_echo)),
        "the fake echoed the received decision `{expected_echo}`, got {messages:?}"
    );
}
