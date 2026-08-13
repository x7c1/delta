//! Transport + demux unit tests, driven by an in-process scripted server over a
//! duplex pipe (no subprocess). The end-to-end test against the real fake
//! app-server binary lives in `fake-codex/tests/`.

use std::sync::Arc;
use std::time::Duration;

use delta_usecase::{
    AgentAdapter, AgentContentSource, AgentEvent, ContentSourceRequest, LaunchOptionSpec,
    LaunchRequest, Message, ResumeRequest, SendRequest, SessionId, ThreadId,
};
use serde_json::{json, Value};
use tokio::io::{split, AsyncBufReadExt, AsyncWriteExt, BufReader, ReadHalf, WriteHalf};
use tokio::sync::mpsc;

use super::*;
use crate::adapter::{thread_resume_params, thread_start_params};

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

/// The unrouted channel is bounded (see `UNROUTED_CAPACITY`), so it has its own
/// receiver type.
async fn recv_unrouted(rx: &mut mpsc::Receiver<ThreadEvent>) -> ThreadEvent {
    tokio::time::timeout(TIMEOUT, rx.recv())
        .await
        .expect("timed out waiting for an unrouted frame")
        .expect("unrouted channel closed")
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
            launch_options: Vec::new(),
            first_prompt: None,
            worktree_repo_root: None,
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
            launch_options: Vec::new(),
            first_prompt: None,
            worktree_repo_root: None,
        })
        .await
        .expect("launch");
    adapter
        .interrupt(&handle)
        .await
        .expect("interrupt is a no-op");

    server_task.await.unwrap();
}

// --- Connection death -------------------------------------------------------

/// A dying connection announces itself on **every** subscribed thread, rather
/// than letting their channels fall silent.
///
/// Silence is the failure this closes: a session whose stream simply stops is
/// indistinguishable from one whose model is thinking, so its turn and its
/// pending approvals waited forever.
#[tokio::test]
async fn a_dying_connection_announces_itself_on_every_subscribed_thread() {
    let (conn, server) = connect();
    let mut first = conn.subscribe_thread("thr_one");
    let mut second = conn.subscribe_thread("thr_two");

    // The server process goes away: its stdout closes (EOF at our reader).
    drop(server);

    assert_eq!(recv(&mut first).await, ThreadEvent::ConnectionLost);
    assert_eq!(recv(&mut second).await, ThreadEvent::ConnectionLost);
}

/// A thread that subscribes *after* the connection died learns so immediately,
/// from the backlog, instead of waiting for a frame that can never arrive.
#[tokio::test]
async fn a_thread_that_died_before_subscribing_reports_the_loss_on_subscribe() {
    let (conn, mut server) = connect();
    // A frame arrives for a thread nobody has subscribed to yet (it is buffered
    // against that thread), and then the server dies.
    server
        .send(json!({
            "method": "turn/started",
            "params": {
                "threadId": "thr_late",
                "turn": { "id": "turn_late", "status": "inProgress", "items": [] }
            }
        }))
        .await;
    drop(server);

    // Wait until the reader has finished: it drops the unrouted sender as it
    // returns, which is strictly after it announced the loss, so subscribing
    // below observes a complete backlog rather than racing it.
    let mut unrouted = conn.take_unrouted().expect("the unrouted channel is free");
    tokio::time::timeout(TIMEOUT, async { while unrouted.recv().await.is_some() {} })
        .await
        .expect("the reader task exits once the server is gone");

    let mut events = conn.subscribe_thread("thr_late");
    assert!(
        matches!(
            events.try_recv(),
            Ok(ThreadEvent::Notification(ref n)) if n.method == "turn/started"
        ),
        "the frame that arrived before the death is delivered first"
    );
    assert_eq!(
        events.try_recv(),
        Ok(ThreadEvent::ConnectionLost),
        "the death is the last thing the backlog carries"
    );
}

/// The adapter turns a connection death into the terminal neutral event on the
/// session's own stream: `SessionEnded { ProcessExited }` — the fact the core
/// settles the stuck turn and its pending approvals on.
#[tokio::test]
async fn a_dead_connection_ends_the_adapters_session_as_process_exited() {
    let (conn, mut server) = connect();
    let adapter = CodexAppServerAdapter::new(Arc::new(conn));

    let server_task = tokio::spawn(async move {
        let start = server.next_frame().await;
        assert_eq!(start["method"], "thread/start");
        server
            .send(json!({ "id": start["id"], "result": { "thread": { "id": "thr_dead" } } }))
            .await;
        server
    });
    let handle = adapter
        .launch(LaunchRequest {
            session_id: "01920000-0000-7000-8000-00000000000b".to_owned(),
            workdir: "/tmp/workdir".to_owned(),
            launch_options: Vec::new(),
            first_prompt: None,
            worktree_repo_root: None,
        })
        .await
        .expect("launch");
    let mut events = adapter.events(&handle);
    let server = tokio::time::timeout(TIMEOUT, server_task)
        .await
        .expect("server task timed out")
        .expect("server task panicked");

    // The app-server process exits.
    drop(server);

    let mut seen = Vec::new();
    let ended = tokio::time::timeout(TIMEOUT, async {
        while let Some(event) = events.recv().await {
            if let AgentEvent::SessionEnded { reason } = event {
                return Some(reason);
            }
            seen.push(event);
        }
        None
    })
    .await
    .expect("the session stream reports the death rather than going quiet");
    assert_eq!(
        ended,
        Some(delta_usecase::SessionEndReason::ProcessExited),
        "a death is reported as an exit, not as an orderly close; saw {seen:?} first"
    );
}

/// A death also **releases the session's comms-log buffer**, exactly as `close`
/// does on the orderly path.
///
/// It has to happen here: the core settles a death by dropping the session's
/// adapter, never by calling `close`, so nothing else would ever release it. And
/// the log's contract is that a session's frames go when the session closes (a
/// reconnect then shows an empty log), while its own pruning reclaims only
/// buffers that are both empty and unwatched — so a dead session's frames, and
/// the browsers still subscribed to them, would otherwise be held for the whole
/// process's lifetime.
#[tokio::test]
async fn a_dead_connection_releases_its_sessions_comms_log() {
    #[derive(Default)]
    struct RecordingSink {
        discarded: std::sync::Mutex<Vec<String>>,
    }
    impl CommsLogSink for RecordingSink {
        fn record(&self, _session_id: &str, _entry: CommsEntry) {}
        fn discard(&self, session_id: &str) {
            self.discarded.lock().unwrap().push(session_id.to_owned());
        }
    }

    const SESSION_ID: &str = "01920000-0000-7000-8000-00000000000d";
    let sink = Arc::new(RecordingSink::default());
    let (conn, mut server) = connect();
    let adapter = CodexAppServerAdapter::new(Arc::new(conn.with_comms_log(sink.clone())));

    let server_task = tokio::spawn(async move {
        let start = server.next_frame().await;
        server
            .send(json!({ "id": start["id"], "result": { "thread": { "id": "thr_logged" } } }))
            .await;
        server
    });
    let handle = adapter
        .launch(LaunchRequest {
            session_id: SESSION_ID.to_owned(),
            workdir: "/tmp/workdir".to_owned(),
            launch_options: Vec::new(),
            first_prompt: None,
            worktree_repo_root: None,
        })
        .await
        .expect("launch");
    let _events = adapter.events(&handle);
    let server = tokio::time::timeout(TIMEOUT, server_task)
        .await
        .expect("server task timed out")
        .expect("server task panicked");
    assert!(
        sink.discarded.lock().unwrap().is_empty(),
        "a live session keeps its log"
    );

    // The app-server process exits.
    drop(server);

    // The release happens just after the terminal event is pushed, so poll for
    // it rather than assuming the translation task has already run.
    tokio::time::timeout(TIMEOUT, async {
        while sink.discarded.lock().unwrap().is_empty() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("a died session's comms log is released");
    assert_eq!(
        *sink.discarded.lock().unwrap(),
        vec![SESSION_ID.to_owned()],
        "released once, under Delta's own session id (the key the browser asks by)"
    );
}

/// An orderly `close` is unchanged and still reports itself as a *close*: the
/// session ends once, as `Closed`, and the failure variant never follows — even
/// though the connection behind it does die afterwards (which is what dropping a
/// closed session's plumbing means).
#[tokio::test]
async fn an_orderly_close_ends_the_session_as_closed_and_never_as_a_failure() {
    let (conn, mut server) = connect();
    let adapter = CodexAppServerAdapter::new(Arc::new(conn));

    let server_task = tokio::spawn(async move {
        let start = server.next_frame().await;
        server
            .send(json!({ "id": start["id"], "result": { "thread": { "id": "thr_closed" } } }))
            .await;
        server
    });
    let handle = adapter
        .launch(LaunchRequest {
            session_id: "01920000-0000-7000-8000-00000000000c".to_owned(),
            workdir: "/tmp/workdir".to_owned(),
            launch_options: Vec::new(),
            first_prompt: None,
            worktree_repo_root: None,
        })
        .await
        .expect("launch");
    let mut events = adapter.events(&handle);
    let server = tokio::time::timeout(TIMEOUT, server_task)
        .await
        .expect("server task timed out")
        .expect("server task panicked");

    adapter.close(&handle).await.expect("close");
    // The connection goes away after the close, as it does in production (the
    // core drops the session's adapter once it is closed).
    drop(server);

    let mut ends = Vec::new();
    tokio::time::timeout(TIMEOUT, async {
        while let Some(event) = events.recv().await {
            if let AgentEvent::SessionEnded { reason } = event {
                ends.push(reason);
            }
        }
    })
    .await
    .expect("a closed session's stream ends rather than hanging");
    assert_eq!(
        ends,
        vec![delta_usecase::SessionEndReason::Closed],
        "exactly one end, and it is the orderly one"
    );
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

    let err = tokio::time::timeout(TIMEOUT, conn.request(None, "turn/start", None))
        .await
        .expect("request should not hang after the connection closes")
        .unwrap_err();
    assert!(matches!(err, Error::ConnectionClosed));
    server_task.await.unwrap();
}

// --- Launch options → `thread/start` fields ---------------------------------

fn option(name: &str, value: Option<&str>) -> LaunchOptionSpec {
    LaunchOptionSpec {
        name: name.to_owned(),
        value: value.map(str::to_owned),
    }
}

/// The mapping rule: a launch option's `name` is the `thread/start` field and
/// its `value` is that field's value, passed through with no allowlist. A value
/// that is not valid JSON is the string it looks like; one that parses keeps its
/// real type (so a boolean/object/number field works); a valueless option is the
/// bare boolean `true`.
#[test]
fn launch_options_map_onto_thread_start_fields_by_name() {
    let params = thread_start_params(
        "/work",
        &[
            option("model", Some("gpt-5.6-sol")),
            option("sandbox", Some("read-only")),
            option("ephemeral", None),
            option("config", Some(r#"{"tools":{"web_search":true}}"#)),
            option("approvalPolicy", Some(r#"{"granular":{"edits":"never"}}"#)),
        ],
        None,
    )
    .expect("no delta-owned key is used");

    assert_eq!(
        params,
        json!({
            // Delta's own field is always present.
            "cwd": "/work",
            // Not valid JSON → the string itself.
            "model": "gpt-5.6-sol",
            "sandbox": "read-only",
            // Valueless → the field is switched on.
            "ephemeral": true,
            // Valid JSON → the parsed value, with its real type.
            "config": { "tools": { "web_search": true } },
            "approvalPolicy": { "granular": { "edits": "never" } },
        })
    );
}

/// A value that happens to parse as a JSON scalar keeps that type, and a quoted
/// value is the escape hatch for wanting the literal string back.
#[test]
fn a_json_scalar_value_keeps_its_type_and_a_quoted_value_stays_a_string() {
    let params = thread_start_params(
        "/work",
        &[
            option("ephemeral", Some("false")),
            option("serviceTier", Some(r#""5""#)),
        ],
        None,
    )
    .expect("no delta-owned key is used");

    assert_eq!(params["ephemeral"], json!(false));
    assert_eq!(params["serviceTier"], json!("5"));
}

/// A launch option naming a field Delta fills in itself is rejected, naming the
/// offending key — never silently dropped, and never allowed to overwrite the
/// value Delta recorded the session against.
#[test]
fn a_launch_option_naming_a_delta_owned_field_is_rejected() {
    let err = thread_start_params("/work", &[option("cwd", Some("/somewhere/else"))], None)
        .expect_err("cwd is Delta's to set");

    let message = err.to_string();
    assert!(
        message.contains("cwd"),
        "the error names the offending key, got: {message}"
    );
}

/// Two selected options naming the same field are rejected rather than one
/// silently winning: a JSON field, unlike a repeatable CLI flag, can only be set
/// once.
#[test]
fn two_launch_options_naming_the_same_field_are_rejected() {
    let err = thread_start_params(
        "/work",
        &[
            option("model", Some("gpt-5.6-sol")),
            option("model", Some("o3")),
        ],
        None,
    )
    .expect_err("a duplicate field is ambiguous");

    let message = err.to_string();
    assert!(
        message.contains("model"),
        "the error names the duplicated key, got: {message}"
    );
}

/// With nothing selected the launch is byte-identical to what it was before
/// launch options existed: `cwd` alone.
#[test]
fn no_launch_options_leaves_thread_start_params_as_just_the_workdir() {
    let params = thread_start_params("/work", &[], None).expect("no options to reject");
    assert_eq!(params, json!({ "cwd": "/work" }));
}

// --- The worktree git-directory grant ---------------------------------------

/// The dotted config key the grant rides, spelled out here so a test failure
/// shows the exact wire key rather than a constant's name.
const GRANT_KEY: &str = "sandbox_workspace_write.writable_roots";

/// A session launched in a Delta-created worktree grants that worktree's REAL
/// git directory — the source repository's `.git`, where git's writes actually
/// land — to Codex's `workspace-write` sandbox.
///
/// The worktree's own `<cwd>/.git` is deliberately NOT the granted path: it is
/// only a pointer file, so granting it would leave every `git add` in the
/// session escalating for approval exactly as before.
#[test]
fn a_worktree_launch_grants_the_source_repositorys_git_directory() {
    let params = thread_start_params("/worktrees/org-repo-01", &[], Some("/repos/org/repo"))
        .expect("no options to reject");

    assert_eq!(
        params,
        json!({
            "cwd": "/worktrees/org-repo-01",
            "config": { GRANT_KEY: ["/repos/org/repo/.git"] },
        })
    );
}

/// A launch outside a Delta-created worktree is byte-identical to what it was
/// before the grant existed — no `config` appears, whether or not the user
/// selected options. Whether a plain clone's `.git` should be writable is the
/// user's own global-config choice; Delta only speaks for worktrees it made.
#[test]
fn a_non_worktree_launch_injects_nothing() {
    let bare = thread_start_params("/work", &[], None).expect("no options to reject");
    assert_eq!(bare, json!({ "cwd": "/work" }));

    let with_options = thread_start_params("/work", &[option("model", Some("gpt-5.6-sol"))], None)
        .expect("no options to reject");
    assert_eq!(
        with_options,
        json!({ "cwd": "/work", "model": "gpt-5.6-sol" })
    );
}

/// A resume re-supplies the same grant a fresh start got, so a worktree session
/// reattached after a restart is sandboxed exactly like it was launched.
#[test]
fn a_worktree_resume_grants_the_same_git_directory() {
    let params = thread_resume_params(
        "thr_worktree",
        "/worktrees/org-repo-01",
        Some("/repos/org/repo"),
    );

    assert_eq!(
        params,
        json!({
            "threadId": "thr_worktree",
            "cwd": "/worktrees/org-repo-01",
            "config": { GRANT_KEY: ["/repos/org/repo/.git"] },
        })
    );
}

/// Resuming a non-worktree session sends what it always sent: the thread and the
/// directory to reattach in.
#[test]
fn a_non_worktree_resume_injects_nothing() {
    let params = thread_resume_params("thr_plain", "/work", None);
    assert_eq!(params, json!({ "threadId": "thr_plain", "cwd": "/work" }));
}

/// A user-registered `config` that says nothing about the sandbox is **merged**
/// with, not replaced by, the grant: every key they registered survives and the
/// grant is added alongside.
#[test]
fn a_user_config_without_a_sandbox_key_is_merged_with_the_grant() {
    let params = thread_start_params(
        "/worktrees/org-repo-01",
        &[option(
            "config",
            Some(r#"{"tools":{"web_search":true},"model_reasoning_effort":"high"}"#),
        )],
        Some("/repos/org/repo"),
    )
    .expect("no delta-owned key is used");

    assert_eq!(
        params,
        json!({
            "cwd": "/worktrees/org-repo-01",
            "config": {
                "tools": { "web_search": true },
                "model_reasoning_effort": "high",
                GRANT_KEY: ["/repos/org/repo/.git"],
            },
        })
    );
}

/// A user-registered `config` that states its own `sandbox_workspace_write`
/// suppresses the grant entirely and passes through verbatim — in BOTH spellings
/// the config format allows, the dotted key and the nested table.
///
/// Delta never rewrites an explicit sandbox setting: the alternative is
/// re-implementing Codex's own merge semantics over a value the user wrote
/// deliberately. The cost is that such a session keeps the approval prompts this
/// feature removes — which is the visible status quo, not a new failure.
#[test]
fn a_user_config_stating_the_sandbox_suppresses_the_grant() {
    let dotted = thread_start_params(
        "/worktrees/org-repo-01",
        &[option(
            "config",
            Some(r#"{"sandbox_workspace_write.writable_roots":["/elsewhere"]}"#),
        )],
        Some("/repos/org/repo"),
    )
    .expect("no delta-owned key is used");
    assert_eq!(
        dotted,
        json!({
            "cwd": "/worktrees/org-repo-01",
            "config": { GRANT_KEY: ["/elsewhere"] },
        }),
        "the user's dotted sandbox key passes through untouched"
    );

    let nested = thread_start_params(
        "/worktrees/org-repo-01",
        &[option(
            "config",
            Some(r#"{"sandbox_workspace_write":{"network_access":false}}"#),
        )],
        Some("/repos/org/repo"),
    )
    .expect("no delta-owned key is used");
    assert_eq!(
        nested,
        json!({
            "cwd": "/worktrees/org-repo-01",
            "config": { "sandbox_workspace_write": { "network_access": false } },
        }),
        "a nested sandbox table also passes through untouched, even though the \
         key Delta would add is not the one it sets"
    );
}

/// A `config` launch option that is not an object at all (the registry stores
/// text, and the value is passed through unvalidated) is left for the server to
/// reject: Delta has nothing to merge into, so it defers rather than replacing
/// what the user typed.
#[test]
fn a_non_object_user_config_suppresses_the_grant() {
    let params = thread_start_params(
        "/worktrees/org-repo-01",
        &[option("config", Some("not-an-object"))],
        Some("/repos/org/repo"),
    )
    .expect("no delta-owned key is used");

    assert_eq!(
        params,
        json!({ "cwd": "/worktrees/org-repo-01", "config": "not-an-object" })
    );
}

/// The grant does not soften the duplicate-key rejection for the field it merges
/// into: two selected options both named `config` are still rejected, worktree or
/// not, because the second would silently discard the first.
#[test]
fn two_config_launch_options_are_still_rejected_in_a_worktree() {
    let err = thread_start_params(
        "/worktrees/org-repo-01",
        &[
            option("config", Some(r#"{"tools":{"web_search":true}}"#)),
            option("config", Some(r#"{"model_reasoning_effort":"high"}"#)),
        ],
        Some("/repos/org/repo"),
    )
    .expect_err("a duplicate field is ambiguous");

    let message = err.to_string();
    assert!(
        message.contains("config"),
        "the error names the duplicated key, got: {message}"
    );
}

/// A content-source request for a session launched in `/work/app` on `branch` —
/// the neutral half of what the pump hands the adapter at bind time. It carries
/// the launch site Delta resolved and observed; the model the server reported is
/// the adapter's to add.
fn content_request(branch: Option<&str>) -> ContentSourceRequest {
    ContentSourceRequest {
        session_id: SessionId::from("01920000-0000-7000-8000-00000000000a"),
        main_thread: ThreadId(1),
        seed_seq: 0,
        cwd: "/work/app".to_owned(),
        git_branch: branch.map(str::to_owned),
    }
}

/// The one assistant message the source folds from a single completed item, for
/// asserting the provider metadata stamped on it.
fn folded_message(source: &mut Box<dyn AgentContentSource>) -> Message {
    let (messages, _) = source.ingest(&AgentEvent::AssistantMessage {
        provider_item_id: "item_1".to_owned(),
        text: "hi".to_owned(),
        at_ms: None,
    });
    messages.into_iter().next().expect("one folded message")
}

/// A launched session's messages report the model the **server** resolved, not
/// the one Delta asked for.
///
/// The launch selects one model and the server answers with a **different** one
/// — exactly what happens when the user's own Codex config or the server's
/// default wins over (or renames) the requested model. That divergence is the
/// point of the test, so the requested value is deliberately a synthetic string
/// that no catalog contains (`requested-by-delta`): were it a real slug, a
/// future edit could quietly align it with the resolved one and the test would
/// keep passing while proving nothing. The value that reaches the message is the
/// server's, so the transcript always shows what is really running.
///
/// `cwd` and `git_branch` ride the neutral request — Codex reports neither, so
/// they are Delta's own launch site — and `response_time_ms` stays `None`, since
/// Codex exposes no per-message latency.
#[tokio::test]
async fn a_launched_sessions_messages_carry_the_model_the_server_resolved() {
    let (conn, mut server) = connect();
    let adapter = CodexAppServerAdapter::new(Arc::new(conn));

    let server_task = tokio::spawn(async move {
        let start = server.next_frame().await;
        assert_eq!(start["method"], "thread/start");
        assert_eq!(
            start["params"]["model"], "requested-by-delta",
            "the selected launch option rode the request"
        );
        // The real `ThreadStartResponse` carries `model` at the top level.
        // Answer with a DIFFERENT model than was requested.
        server
            .send(json!({
                "id": start["id"],
                "result": { "thread": { "id": "thr_m" }, "model": "gpt-5.6-sol" }
            }))
            .await;
        // Hold the server side open until the test drops it.
        server
    });

    let handle = adapter
        .launch(LaunchRequest {
            session_id: "01920000-0000-7000-8000-00000000000a".to_owned(),
            workdir: "/work/app".to_owned(),
            launch_options: vec![option("model", Some("requested-by-delta"))],
            first_prompt: None,
            worktree_repo_root: None,
        })
        .await
        .expect("launch");
    let _server = tokio::time::timeout(TIMEOUT, server_task)
        .await
        .expect("server task timed out")
        .expect("server task panicked");

    let mut source = adapter.content_source(&handle, content_request(Some("feature/x")));
    let message = folded_message(&mut source);
    assert_eq!(
        message.model.as_deref(),
        Some("gpt-5.6-sol"),
        "the message reports the model the server resolved, never the requested one"
    );
    assert_eq!(
        message.git_branch.as_deref(),
        Some("feature/x"),
        "the launch site's branch rides the neutral request onto the message"
    );
    assert_eq!(message.cwd.as_deref(), Some("/work/app"));
    assert!(
        message.response_time_ms.is_none(),
        "Codex exposes no per-message latency, so it degrades to None"
    );
}

/// A **resumed** session reports its metadata too: `thread/resume` carries the
/// same required top-level `model` as `thread/start`, so a session picked back
/// up after a restart is not left blank.
#[tokio::test]
async fn a_resumed_sessions_messages_carry_the_model_the_resume_reported() {
    let (conn, mut server) = connect();
    let adapter = CodexAppServerAdapter::new(Arc::new(conn));

    let server_task = tokio::spawn(async move {
        let resume = server.next_frame().await;
        assert_eq!(resume["method"], "thread/resume");
        assert_eq!(resume["params"]["threadId"], "thr_m");
        server
            .send(json!({
                "id": resume["id"],
                "result": { "thread": { "id": "thr_m" }, "model": "gpt-5.6-sol" }
            }))
            .await;
        server
    });

    let handle = adapter
        .resume(ResumeRequest {
            session_id: "01920000-0000-7000-8000-00000000000a".to_owned(),
            provider_session_id: "thr_m".to_owned(),
            workdir: "/work/app".to_owned(),
            worktree_repo_root: None,
        })
        .await
        .expect("resume");
    let _server = tokio::time::timeout(TIMEOUT, server_task)
        .await
        .expect("server task timed out")
        .expect("server task panicked");

    let mut source = adapter.content_source(&handle, content_request(Some("feature/x")));
    let message = folded_message(&mut source);
    assert_eq!(
        message.model.as_deref(),
        Some("gpt-5.6-sol"),
        "a resumed session reports the model its resume response announced"
    );
    assert_eq!(
        message.git_branch.as_deref(),
        Some("feature/x"),
        "a resumed session's launch site is re-observed and reported too"
    );
    assert_eq!(message.cwd.as_deref(), Some("/work/app"));
}

/// A server answering without a `model`, for a session launched outside a git
/// working tree, leaves both facts absent rather than substituting something
/// plausible — the same degrade-never-fake rule the rest of the Codex fold
/// follows. The launch directory, which Delta always knows, is still reported.
#[tokio::test]
async fn a_response_without_a_model_degrades_to_none() {
    let (conn, mut server) = connect();
    let adapter = CodexAppServerAdapter::new(Arc::new(conn));

    let server_task = tokio::spawn(async move {
        let start = server.next_frame().await;
        server
            .send(json!({ "id": start["id"], "result": { "thread": { "id": "thr_n" } } }))
            .await;
        server
    });

    let handle = adapter
        .launch(LaunchRequest {
            session_id: "01920000-0000-7000-8000-00000000000a".to_owned(),
            workdir: "/work/app".to_owned(),
            launch_options: Vec::new(),
            first_prompt: None,
            worktree_repo_root: None,
        })
        .await
        .expect("launch");
    let _server = tokio::time::timeout(TIMEOUT, server_task)
        .await
        .expect("server task timed out")
        .expect("server task panicked");

    let mut source = adapter.content_source(&handle, content_request(None));
    let message = folded_message(&mut source);
    assert!(
        message.model.is_none(),
        "no model in the response means no model on the message"
    );
    assert!(
        message.git_branch.is_none(),
        "no branch observed at the launch site means none on the message"
    );
    assert_eq!(
        message.cwd.as_deref(),
        Some("/work/app"),
        "the launch directory is Delta's own record, so it is reported regardless"
    );
}
