//! End-to-end transport proof: the real [`codex_agent`] client, spawned against
//! the built `fake-codex` binary, completes the `initialize` handshake and a
//! scripted `thread/start` round-trip, and the turn's `item/*` / `turn/*`
//! notifications are demuxed onto the started thread's channel.
//!
//! This is the C1 exit-gate test that proves transport + demux works over a real
//! subprocess and real stdio pipes (the in-process unit tests cover the framing
//! and correlation logic in `codex-agent` itself).

use std::time::Duration;

use codex_agent::{AppServerConnection, CodexLaunchConfig, ThreadEvent};
use serde_json::json;
use tokio::sync::mpsc::UnboundedReceiver;

const TIMEOUT: Duration = Duration::from_secs(10);

/// Point the client's launch config at the built `fake-codex` binary (no args —
/// the fake is the server itself, not `codex app-server`).
fn fake_config() -> CodexLaunchConfig {
    CodexLaunchConfig {
        codex_bin: env!("CARGO_BIN_EXE_fake-codex").to_owned(),
        args: vec![],
        env: vec![],
    }
}

async fn recv(rx: &mut UnboundedReceiver<ThreadEvent>) -> ThreadEvent {
    tokio::time::timeout(TIMEOUT, rx.recv())
        .await
        .expect("timed out waiting for a thread event")
        .expect("thread channel closed")
}

#[tokio::test]
async fn handshake_and_thread_start_round_trip_against_the_fake_binary() {
    let conn = AppServerConnection::spawn(&fake_config()).expect("spawn fake-codex");

    // 1. initialize -> initialized handshake.
    let init = tokio::time::timeout(
        TIMEOUT,
        conn.initialize(json!({ "clientInfo": { "name": "delta" } })),
    )
    .await
    .expect("initialize timed out")
    .expect("initialize failed");
    assert_eq!(init["serverInfo"]["name"], "fake-codex");

    // 2. thread/start round-trip: the fake returns its default thread id.
    let mut started = tokio::time::timeout(TIMEOUT, conn.start_thread(None))
        .await
        .expect("thread/start timed out")
        .expect("thread/start failed");
    assert_eq!(started.thread_id, "thr_fake_0001");

    // 3. turn/start -> the default scenario's turn plays, and its notifications
    //    are demuxed onto this thread's channel in order.
    let turn = tokio::time::timeout(
        TIMEOUT,
        conn.request("turn/start", Some(json!({ "threadId": started.thread_id }))),
    )
    .await
    .expect("turn/start timed out")
    .expect("turn/start failed");
    // Real `turn/start` returns the started turn under `result.turn`.
    assert_eq!(turn["turn"]["id"], "turn_fake_0001");

    let mut methods = Vec::new();
    // The default turn emits: turn/started, item/started, item/completed,
    // turn/completed — four thread-scoped notifications.
    for _ in 0..4 {
        match recv(&mut started.events).await {
            ThreadEvent::Notification(n) => {
                assert_eq!(
                    n.params["threadId"], "thr_fake_0001",
                    "every emitted notification is stamped with the thread id"
                );
                methods.push(n.method);
            }
            other => panic!("expected a notification, got {other:?}"),
        }
    }
    assert_eq!(
        methods,
        vec![
            "turn/started".to_owned(),
            "item/started".to_owned(),
            "item/completed".to_owned(),
            "turn/completed".to_owned(),
        ]
    );
}

#[tokio::test]
async fn scripted_scenario_can_emit_an_approval_request_and_interrupt() {
    // A scenario file exercising the approval-request server->client path.
    let dir = std::env::temp_dir().join(format!("fake-codex-scenario-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let scenario_path = dir.join("scenario.json");
    std::fs::write(
        &scenario_path,
        r#"{
            "thread_id": "thr_script",
            "turn": {
                "turn_id": "turn_script",
                "emit": [
                    { "type": "turn_started" },
                    { "type": "item_started", "item": { "id": "i1", "itemType": "command_execution" } },
                    { "type": "request_approval", "params": { "toolName": "Bash" } },
                    { "type": "item_completed", "item": { "id": "i1" } },
                    { "type": "turn_completed", "status": "completed" }
                ]
            }
        }"#,
    )
    .unwrap();

    let mut config = fake_config();
    // Hand the fake its scenario via the child's env (not the parent process's,
    // which the concurrently-running tests share).
    config.env = vec![(
        "FAKE_CODEX_SCENARIO".to_owned(),
        scenario_path.to_string_lossy().into_owned(),
    )];

    let conn = AppServerConnection::spawn(&config).expect("spawn fake-codex");
    conn.initialize(json!({})).await.expect("initialize failed");
    let mut started = conn.start_thread(None).await.expect("thread/start failed");
    assert_eq!(started.thread_id, "thr_script");

    conn.request("turn/start", Some(json!({ "threadId": started.thread_id })))
        .await
        .expect("turn/start failed");

    // Collect the five emissions; the third is the server-originated approval
    // request, the rest are notifications.
    let mut saw_approval = false;
    let mut saw_turn_completed = false;
    for _ in 0..5 {
        match recv(&mut started.events).await {
            ThreadEvent::ServerRequest(r) => {
                assert_eq!(r.method, "item/requestApproval");
                assert_eq!(r.params["threadId"], "thr_script");
                assert_eq!(r.params["toolName"], "Bash");
                saw_approval = true;
            }
            ThreadEvent::Notification(n) if n.method == "turn/completed" => {
                saw_turn_completed = true;
            }
            ThreadEvent::Notification(_) => {}
        }
    }
    assert!(saw_approval, "the scripted approval request was demuxed");
    assert!(saw_turn_completed, "the turn completed");

    std::fs::remove_dir_all(&dir).ok();
}
