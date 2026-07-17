//! Transport + demux unit tests, driven by an in-process scripted server over a
//! duplex pipe (no subprocess). The end-to-end test against the real fake
//! app-server binary lives in `fake-codex/tests/`.

use std::sync::Arc;
use std::time::Duration;

use delta_usecase::{AgentAdapter, LaunchRequest, SendRequest};
use serde_json::{json, Value};
use tokio::io::{split, AsyncBufReadExt, AsyncWriteExt, BufReader, ReadHalf, WriteHalf};
use tokio::sync::mpsc;

use super::*;

/// A short bound so a wiring bug fails fast instead of hanging the suite.
const TIMEOUT: Duration = Duration::from_secs(5);

/// The server side of a duplex pipe: read client frames line by line, write
/// server frames back.
struct ServerSide {
    reader: BufReader<ReadHalf<tokio::io::DuplexStream>>,
    writer: WriteHalf<tokio::io::DuplexStream>,
}

impl ServerSide {
    async fn next_frame(&mut self) -> Value {
        let mut line = String::new();
        let n = self.reader.read_line(&mut line).await.unwrap();
        assert_ne!(n, 0, "client closed before the expected frame");
        serde_json::from_str(&line).unwrap()
    }

    async fn send(&mut self, frame: Value) {
        let mut line = serde_json::to_string(&frame).unwrap();
        line.push('\n');
        self.writer.write_all(line.as_bytes()).await.unwrap();
        self.writer.flush().await.unwrap();
    }
}

/// Wire a connection to a fresh `ServerSide` over a duplex pipe.
fn connect() -> (AppServerConnection, ServerSide) {
    let (client, server) = tokio::io::duplex(64 * 1024);
    let (client_r, client_w): (ReadHalf<_>, WriteHalf<_>) = split(client);
    let (server_r, server_w) = split(server);
    let conn = AppServerConnection::from_io(client_r, client_w, None);
    (
        conn,
        ServerSide {
            reader: BufReader::new(server_r),
            writer: server_w,
        },
    )
}

async fn recv(rx: &mut mpsc::UnboundedReceiver<ThreadEvent>) -> ThreadEvent {
    tokio::time::timeout(TIMEOUT, rx.recv())
        .await
        .expect("timed out waiting for a thread event")
        .expect("thread channel closed")
}

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

    let mut started = tokio::time::timeout(TIMEOUT, conn.start_thread(None))
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
        conn.request("method/one", None),
        conn.request("method/two", None),
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

    let err = conn.request("does/not/exist", None).await.unwrap_err();
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

    let event = recv(&mut unrouted).await;
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

/// The adapter captures the current turn's id from the `turn/start` response
/// (`result.turn.id`) and sends it back in `turn/interrupt`'s params
/// (`{threadId, turnId}`) — the reconciled interrupt shape. Also asserts the
/// reconciled `turn/start` `input` array shape rides the wire.
#[tokio::test]
async fn adapter_captures_turn_id_and_sends_it_on_interrupt() {
    let (conn, mut server) = connect();
    let adapter = CodexAppServerAdapter::new(Arc::new(conn));

    let server_task = tokio::spawn(async move {
        // launch -> thread/start, answered with the reconciled `{thread:{id}}`.
        let start = server.next_frame().await;
        assert_eq!(start["method"], "thread/start");
        server
            .send(json!({ "id": start["id"], "result": { "thread": { "id": "thr_u" } } }))
            .await;

        // send -> turn/start. The visible prompt rides as a `TextUserInput` array.
        let turn = server.next_frame().await;
        assert_eq!(turn["method"], "turn/start");
        assert_eq!(turn["params"]["input"][0]["type"], "text");
        assert_eq!(turn["params"]["input"][0]["text"], "hello");
        // Real `turn/start` returns the started turn under `result.turn`.
        server
            .send(json!({
                "id": turn["id"],
                "result": { "turn": { "id": "turn_u", "status": "inProgress", "items": [] } }
            }))
            .await;

        // interrupt -> turn/interrupt, carrying the tracked turn id.
        let interrupt = server.next_frame().await;
        assert_eq!(interrupt["method"], "turn/interrupt");
        server
            .send(json!({ "id": interrupt["id"], "result": {} }))
            .await;
        interrupt
    });

    let handle = adapter
        .launch(LaunchRequest {
            session_id: "01920000-0000-7000-8000-000000000009".to_owned(),
            workdir: "/tmp/workdir".to_owned(),
            extra_args: Vec::new(),
            first_prompt: None,
        })
        .await
        .expect("launch");
    assert_eq!(handle.provider_session_id, "thr_u");

    let receipt = adapter
        .send(
            &handle,
            SendRequest {
                text: "hello".to_owned(),
            },
        )
        .await
        .expect("send");
    assert_eq!(
        receipt.provider_message_id.as_deref(),
        Some("turn_u"),
        "the send receipt carries the started turn id"
    );

    adapter.interrupt(&handle).await.expect("interrupt");

    let interrupt = tokio::time::timeout(TIMEOUT, server_task)
        .await
        .expect("server task timed out")
        .expect("server task panicked");
    assert_eq!(interrupt["params"]["threadId"], "thr_u");
    assert_eq!(
        interrupt["params"]["turnId"], "turn_u",
        "turn/interrupt must reference the tracked turn id"
    );
}

/// Interrupting a session with no turn in flight is a no-op success: there is no
/// turn id to reference, so the adapter does not send an (invalid) `turn/interrupt`
/// the real server would reject for a missing `turnId`.
#[tokio::test]
async fn interrupt_without_an_active_turn_is_a_no_op() {
    let (conn, mut server) = connect();
    let adapter = CodexAppServerAdapter::new(Arc::new(conn));

    let server_task = tokio::spawn(async move {
        let start = server.next_frame().await;
        assert_eq!(start["method"], "thread/start");
        server
            .send(json!({ "id": start["id"], "result": { "thread": { "id": "thr_idle" } } }))
            .await;
        // Return the server so the connection stays open; assert no further frame
        // (an interrupt RPC) arrives.
        let next = tokio::time::timeout(Duration::from_millis(200), server.next_frame()).await;
        assert!(
            next.is_err(),
            "no turn/interrupt frame is sent when no turn is in flight, got {next:?}"
        );
    });

    let handle = adapter
        .launch(LaunchRequest {
            session_id: "01920000-0000-7000-8000-00000000000a".to_owned(),
            workdir: "/tmp/workdir".to_owned(),
            extra_args: Vec::new(),
            first_prompt: None,
        })
        .await
        .expect("launch");
    adapter
        .interrupt(&handle)
        .await
        .expect("interrupt is a no-op");

    server_task.await.unwrap();
}

/// When the server closes its stdout, an outstanding request resolves to
/// `ConnectionClosed` rather than hanging forever.
#[tokio::test]
async fn request_resolves_to_closed_when_the_server_exits() {
    let (conn, mut server) = connect();
    let server_task = tokio::spawn(async move {
        // Read the request, then drop the pipe (EOF) without answering.
        let _req = server.next_frame().await;
        drop(server);
    });

    let err = tokio::time::timeout(TIMEOUT, conn.request("turn/start", None))
        .await
        .expect("request should not hang after the connection closes")
        .unwrap_err();
    assert!(matches!(err, Error::ConnectionClosed));
    server_task.await.unwrap();
}
