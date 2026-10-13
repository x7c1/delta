//! The handshake, request/response correlation and the `threadId` demux.

use super::*;

/// The exit-gate scenario: an `initialize` handshake followed by a scripted
/// `thread/start` round-trip, then a thread-scoped notification is demuxed to
/// that thread's channel. Proves transport + demux end to end in-process.
#[tokio::test]
async fn handshake_then_thread_start_round_trip_and_demux() {
    let (conn, mut server) = connect();

    // Scripted server: answer initialize, swallow the initialized notification,
    // answer thread/start with an id, then push a turn notification for it.
    let server_task = tokio::spawn(async move {
        let init = server.next_frame().await;
        assert_eq!(init["method"], "initialize");
        let init_id = init["id"].clone();
        server
            .send(json!({ "id": init_id, "result": { "serverInfo": { "name": "fake" } } }))
            .await;

        let initialized = server.next_frame().await;
        assert_eq!(initialized["method"], "initialized");
        assert!(initialized.get("id").is_none(), "notifications carry no id");

        let start = server.next_frame().await;
        assert_eq!(start["method"], "thread/start");
        // Real `thread/start` returns the started thread under `result.thread`.
        server
            .send(json!({
                "id": start["id"],
                "result": { "thread": { "id": "thr_round_trip" } }
            }))
            .await;

        // A thread-scoped notification that must land on the thread's channel.
        // Real `turn/completed` wraps the turn under `params.turn`.
        server
            .send(json!({
                "method": "turn/completed",
                "params": {
                    "threadId": "thr_round_trip",
                    "turn": { "id": "turn_round_trip", "status": "completed", "items": [] }
                }
            }))
            .await;
        server
    });

    let init_result = tokio::time::timeout(TIMEOUT, conn.initialize(json!({ "clientInfo": {} })))
        .await
        .expect("initialize timed out")
        .expect("initialize failed");
    assert_eq!(init_result["serverInfo"]["name"], "fake");

    let mut started = tokio::time::timeout(TIMEOUT, conn.start_thread(None, None))
        .await
        .expect("thread/start timed out")
        .expect("thread/start failed");
    assert_eq!(started.thread_id, "thr_round_trip");

    let event = recv(&mut started.events).await;
    match event {
        ThreadEvent::Notification(n) => {
            assert_eq!(n.method, "turn/completed");
            assert_eq!(n.params["turn"]["status"], "completed");
        }
        other => panic!("expected a notification, got {other:?}"),
    }

    // Keep the server task alive until here so its writes are observed.
    server_task.await.unwrap();
}

/// Two in-flight requests are correlated to their own responses even when the
/// server answers them out of order.
#[tokio::test]
async fn correlates_concurrent_requests_answered_out_of_order() {
    let (conn, mut server) = connect();

    let server_task = tokio::spawn(async move {
        let first = server.next_frame().await;
        let second = server.next_frame().await;
        // Answer the second request first, then the first.
        server
            .send(json!({ "id": second["id"], "result": { "which": "second" } }))
            .await;
        server
            .send(json!({ "id": first["id"], "result": { "which": "first" } }))
            .await;
    });

    let (a, b) = tokio::join!(
        conn.request(None, "method/one", None),
        conn.request(None, "method/two", None),
    );
    assert_eq!(a.unwrap()["which"], "first");
    assert_eq!(b.unwrap()["which"], "second");
    server_task.await.unwrap();
}

/// A JSON-RPC error response surfaces as `Error::Rpc` naming the failed method.
#[tokio::test]
async fn error_response_surfaces_as_rpc_error() {
    let (conn, mut server) = connect();
    let server_task = tokio::spawn(async move {
        let req = server.next_frame().await;
        server
            .send(json!({
                "id": req["id"],
                "error": { "code": -32601, "message": "method not found" }
            }))
            .await;
    });

    let err = conn
        .request(None, "does/not/exist", None)
        .await
        .unwrap_err();
    match err {
        Error::Rpc { method, error } => {
            assert_eq!(method, "does/not/exist");
            assert_eq!(error.code, -32601);
        }
        other => panic!("expected an Rpc error, got {other:?}"),
    }
    server_task.await.unwrap();
}

/// A notification with no `threadId` (or for an unregistered thread) is routed
/// to the connection-level unrouted channel rather than dropped.
#[tokio::test]
async fn unscoped_notification_goes_to_the_unrouted_channel() {
    let (conn, mut server) = connect();
    let mut unrouted = conn
        .take_unrouted()
        .expect("unrouted channel is available once");
    assert!(
        conn.take_unrouted().is_none(),
        "the unrouted channel is handed out only once"
    );

    let server_task = tokio::spawn(async move {
        server
            .send(json!({ "method": "server/status", "params": { "ok": true } }))
            .await;
        server
    });

    let event = recv_unrouted(&mut unrouted).await;
    match event {
        ThreadEvent::Notification(n) => assert_eq!(n.method, "server/status"),
        other => panic!("expected a notification, got {other:?}"),
    }
    server_task.await.unwrap();
}

/// A server-originated request scoped to a subscribed thread is demuxed to that
/// thread's channel as a `ServerRequest` (the C2 adapter later answers it).
#[tokio::test]
async fn server_request_is_demuxed_to_the_thread_channel() {
    let (conn, mut server) = connect();
    let mut events = conn.subscribe_thread("thr_approval");

    let server_task = tokio::spawn(async move {
        server
            .send(json!({
                "id": "srv-1",
                "method": "item/commandExecution/requestApproval",
                "params": { "threadId": "thr_approval", "itemId": "exec-1", "command": "date" }
            }))
            .await;
        server
    });

    let event = recv(&mut events).await;
    match event {
        ThreadEvent::ServerRequest(r) => {
            assert_eq!(r.method, "item/commandExecution/requestApproval");
            assert_eq!(r.id, json!("srv-1"));
        }
        other => panic!("expected a server request, got {other:?}"),
    }
    server_task.await.unwrap();
}
