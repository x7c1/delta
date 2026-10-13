//! Connection death.

use super::*;

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
