//! The comms-log tap, driven through [`CodexAppServerAdapter`] against the real
//! `fake-codex` app-server binary.
//!
//! A headless provider's session has no terminal, so the frames it exchanges are
//! the only window into what it is doing. This suite pins that window's contract
//! at the layer that produces it:
//!
//! 1. **Coverage** — a launched, prompted, approved and closed session records
//!    every frame shape the transport carries, in both directions: Delta's
//!    requests and the responses they get, the server's pushed notifications, a
//!    server-originated approval request, and Delta's own answer to it.
//! 2. **Attribution** — frames are recorded under *Delta's* session id (the id
//!    the browser has), not the provider's thread id, and a second session on the
//!    same shared server never lands in the first one's log.
//! 3. **Never blocks** — a full turn completes with nothing consuming the log,
//!    and with a real [`CommsLogHub`] whose live channel is saturated by a
//!    subscriber that never reads. This is the invariant that matters: the whole
//!    reason this provider has no terminal is that a session must never hang
//!    invisibly, so an inspector nobody is reading must not be able to hold a
//!    turn up.
//!
//! That the recording is observability only — that it adds, drops, or reorders
//! no neutral event — is pinned by `full_loop.rs`, which drives the same stack
//! with a real [`CommsLogHub`] wired and asserts its event streams frame for
//! frame.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use codex_agent::{AppServerConnection, CodexAppServerAdapter, CodexLaunchConfig};
use delta_server::CommsLogHub;
use delta_usecase::{
    AgentAdapter, AgentEvent, AgentEventStream, CommsDirection, CommsEntry, CommsFrameKind,
    CommsLogSink, LaunchRequest, PermissionDecision, SendRequest,
};
use serde_json::{json, Value};
use tokio::time::timeout;

/// A short bound so a wiring bug fails fast instead of hanging the suite.
const TIMEOUT: Duration = Duration::from_secs(10);

/// Delta's session id for the session under test — deliberately unlike a Codex
/// thread id, so an assertion on it cannot pass by accident if the two were ever
/// confused.
const SESSION_ID: &str = "01920000-0000-7000-8000-0000000000aa";

// --- Recording sink ----------------------------------------------------------

/// A [`CommsLogSink`] that keeps every call, so a test can assert on exactly
/// what the adapter emitted.
#[derive(Default)]
struct RecordingSink {
    entries: Mutex<Vec<(String, CommsEntry)>>,
    discarded: Mutex<Vec<String>>,
}

impl RecordingSink {
    fn arc() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Every entry recorded for `session_id`, in record order.
    fn entries_for(&self, session_id: &str) -> Vec<CommsEntry> {
        self.entries
            .lock()
            .unwrap()
            .iter()
            .filter(|(id, _)| id == session_id)
            .map(|(_, entry)| entry.clone())
            .collect()
    }

    /// The session ids recording touched at all.
    fn recorded_session_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .entries
            .lock()
            .unwrap()
            .iter()
            .map(|(id, _)| id.clone())
            .collect();
        ids.dedup();
        ids
    }

    fn discarded(&self) -> Vec<String> {
        self.discarded.lock().unwrap().clone()
    }
}

impl CommsLogSink for RecordingSink {
    fn record(&self, session_id: &str, entry: CommsEntry) {
        self.entries
            .lock()
            .unwrap()
            .push((session_id.to_owned(), entry));
    }

    fn discard(&self, session_id: &str) {
        self.discarded.lock().unwrap().push(session_id.to_owned());
    }
}

/// Does any recorded entry match this (direction, kind, method) triple?
fn has_frame(
    entries: &[CommsEntry],
    direction: CommsDirection,
    kind: CommsFrameKind,
    method: Option<&str>,
) -> bool {
    entries.iter().any(|entry| {
        entry.direction == direction && entry.kind == kind && entry.method.as_deref() == method
    })
}

// --- Fixture ----------------------------------------------------------------

/// A scenario file written to a unique temp dir, removed on drop.
struct ScenarioGuard {
    dir: PathBuf,
    path: PathBuf,
}

impl ScenarioGuard {
    fn write(scenario: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("fake-codex-comms-{}-{}", std::process::id(), n));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("scenario.json");
        std::fs::write(&path, scenario).unwrap();
        Self { dir, path }
    }
}

impl Drop for ScenarioGuard {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).ok();
    }
}

/// Spawn the fake with `scenario` and build an initialised adapter whose
/// connection mirrors its frames into `sink`.
async fn adapter_with(
    scenario: &str,
    sink: Arc<dyn CommsLogSink>,
) -> (CodexAppServerAdapter, ScenarioGuard) {
    let guard = ScenarioGuard::write(scenario);
    let config = CodexLaunchConfig {
        codex_bin: env!("CARGO_BIN_EXE_fake-codex").to_owned(),
        args: vec![],
        env: vec![(
            "FAKE_CODEX_SCENARIO".to_owned(),
            guard.path.to_string_lossy().into_owned(),
        )],
    };
    let conn = Arc::new(
        AppServerConnection::spawn(&config)
            .expect("spawn fake-codex")
            .with_comms_log(sink),
    );
    conn.initialize(json!({ "clientInfo": { "name": "delta", "version": "0" } }))
        .await
        .expect("initialize");
    (CodexAppServerAdapter::new(conn), guard)
}

fn launch_request(session_id: &str) -> LaunchRequest {
    LaunchRequest {
        session_id: session_id.to_owned(),
        workdir: "/tmp/workdir".to_owned(),
        launch_options: Vec::new(),
        first_prompt: None,
    }
}

/// Receive events until `stop` matches one (inclusive), panicking on a hang.
async fn collect_until<F>(stream: &mut AgentEventStream, stop: F) -> Vec<AgentEvent>
where
    F: Fn(&AgentEvent) -> bool,
{
    let mut events = Vec::new();
    loop {
        match timeout(TIMEOUT, stream.recv()).await {
            Ok(Some(event)) => {
                let done = stop(&event);
                events.push(event);
                if done {
                    break;
                }
            }
            Ok(None) => break,
            Err(_) => panic!("timed out waiting for an event; collected so far: {events:?}"),
        }
    }
    events
}

fn is_turn_completed(event: &AgentEvent) -> bool {
    matches!(event, AgentEvent::TurnCompleted { .. })
}

/// A turn that emits the pushed flow AND a blocking command-execution approval,
/// so one run exercises every frame shape the transport carries: Delta's
/// requests, the server's responses, its pushed notifications, its own request,
/// and Delta's answer to that request.
fn approval_turn_scenario() -> &'static str {
    r#"{
        "thread_id": "thr_comms",
        "turn": {
            "turn_id": "turn_comms",
            "emit": [
                { "type": "turn_started" },
                { "type": "request_approval", "method": "item/commandExecution/requestApproval", "params": { "itemId": "exec_1", "command": "date" }, "blocking": true },
                { "type": "item_completed", "item": { "id": "m1", "type": "agentMessage", "text": "done" } },
                { "type": "turn_completed", "status": "completed" }
            ]
        }
    }"#
}

// --- Coverage ---------------------------------------------------------------

/// One launched, prompted, approved and closed session records every frame shape
/// in both directions. Asserted as (direction, kind, method) triples rather than
/// on payload text, so the coverage claim does not re-pin the wire shapes that
/// `codex-agent`'s own suite owns.
#[tokio::test(flavor = "multi_thread")]
async fn every_frame_shape_is_recorded_in_both_directions() {
    let sink = RecordingSink::arc();
    let (adapter, _guard) = adapter_with(
        approval_turn_scenario(),
        Arc::clone(&sink) as Arc<dyn CommsLogSink>,
    )
    .await;

    let handle = adapter
        .launch(launch_request(SESSION_ID))
        .await
        .expect("launch");
    let mut stream = adapter.events(&handle);
    adapter
        .send(
            &handle,
            SendRequest {
                text: "run it".to_owned(),
            },
        )
        .await
        .expect("send");

    // The approval blocks the scripted turn, so answering it is what lets the
    // turn finish — and it is what produces Delta's own outgoing response frame.
    let until_permission = collect_until(&mut stream, |event| {
        matches!(event, AgentEvent::PermissionRequested { .. })
    })
    .await;
    let AgentEvent::PermissionRequested { request } =
        until_permission.last().expect("at least one event").clone()
    else {
        panic!("expected a permission request, got {until_permission:?}");
    };
    adapter
        .resolve_permission(&handle, &request.request_id, PermissionDecision::Allow)
        .await
        .expect("resolve permission");
    collect_until(&mut stream, is_turn_completed).await;

    let entries = sink.entries_for(SESSION_ID);

    // Delta → agent requests, each paired with the response it correlates to.
    for method in ["thread/start", "turn/start"] {
        assert!(
            has_frame(
                &entries,
                CommsDirection::ToAgent,
                CommsFrameKind::Request,
                Some(method)
            ),
            "missing the outgoing `{method}` request in {entries:#?}"
        );
        assert!(
            has_frame(
                &entries,
                CommsDirection::FromAgent,
                CommsFrameKind::Response,
                Some(method)
            ),
            "missing the response to `{method}` in {entries:#?}"
        );
    }

    // Agent → Delta pushed notifications (the turn's own flow).
    assert!(
        has_frame(
            &entries,
            CommsDirection::FromAgent,
            CommsFrameKind::Notification,
            Some("turn/started")
        ),
        "missing the pushed `turn/started` notification in {entries:#?}"
    );
    assert!(
        has_frame(
            &entries,
            CommsDirection::FromAgent,
            CommsFrameKind::Notification,
            Some("turn/completed")
        ),
        "missing the pushed `turn/completed` notification in {entries:#?}"
    );

    // A server-originated request: same kind as one of Delta's own, travelling
    // the other way.
    assert!(
        has_frame(
            &entries,
            CommsDirection::FromAgent,
            CommsFrameKind::Request,
            Some("item/commandExecution/requestApproval")
        ),
        "missing the server-originated approval request in {entries:#?}"
    );

    // And Delta's answer to it: an outgoing response, which names no method of
    // its own (the request above it is what it answers).
    let answer = entries
        .iter()
        .find(|entry| {
            entry.direction == CommsDirection::ToAgent
                && entry.kind == CommsFrameKind::Response
                && entry.method.is_none()
        })
        .unwrap_or_else(|| panic!("missing Delta's answer to the approval in {entries:#?}"));
    let payload: Value = serde_json::from_str(&answer.payload_json).expect("the frame is JSON");
    assert_eq!(
        payload["result"]["decision"], "accept",
        "the recorded answer is the decision that actually went over the wire"
    );

    // Every payload is a self-contained JSON document — the inspector can parse
    // any frame it is handed.
    for entry in &entries {
        serde_json::from_str::<Value>(&entry.payload_json)
            .unwrap_or_else(|err| panic!("payload is not JSON ({err}): {entry:?}"));
        assert!(
            !entry.payload_json.ends_with('\n'),
            "the transport's frame terminator is not part of the payload: {entry:?}"
        );
    }
}

/// Frames are attributed to Delta's session id — the id the browser asks for —
/// and never to the provider's thread id.
#[tokio::test(flavor = "multi_thread")]
async fn frames_are_attributed_to_deltas_session_id() {
    let sink = RecordingSink::arc();
    let (adapter, _guard) = adapter_with(
        approval_turn_scenario(),
        Arc::clone(&sink) as Arc<dyn CommsLogSink>,
    )
    .await;

    let handle = adapter
        .launch(launch_request(SESSION_ID))
        .await
        .expect("launch");
    assert_eq!(
        handle.provider_session_id, "thr_comms",
        "the provider id is the thread id (so the assertion below is meaningful)"
    );

    assert_eq!(
        sink.recorded_session_ids(),
        vec![SESSION_ID.to_owned()],
        "the log is keyed by Delta's session id, never the provider's thread id"
    );
    // The session's very first frames are its own launch, so a pane opened right
    // after the spawn shows how the session started rather than starting blank.
    let entries = sink.entries_for(SESSION_ID);
    assert_eq!(entries[0].method.as_deref(), Some("thread/start"));
    assert_eq!(entries[0].direction, CommsDirection::ToAgent);
}

/// An account-scoped frame — one carrying no `threadId`, so the transport can
/// attribute it to no single session — is mirrored into EVERY live session's
/// log.
///
/// It is a fact about the shared connection, and the rate-limit rows it moves
/// are shown in every one of those sessions, so an operator reading any of their
/// inspectors must be able to see the frame that moved them. Recording it
/// nowhere (which is what the connection alone can do with it) would leave the
/// footer changing for no visible reason.
#[tokio::test(flavor = "multi_thread")]
async fn an_account_scoped_frame_is_recorded_in_every_live_session() {
    let sink = RecordingSink::arc();
    // `account_notification` is emitted verbatim, with no `threadId` stamped in
    // — the way the real server emits an account rolling update.
    let (adapter, _guard) = adapter_with(
        r#"{
            "thread_id": "thr_account",
            "turn": {
                "turn_id": "turn_account",
                "emit": [
                    { "type": "turn_started" },
                    { "type": "account_notification", "method": "account/rateLimits/updated",
                      "params": { "rateLimits": { "primary": { "usedPercent": 21, "windowDurationMins": 300 } } } },
                    { "type": "turn_completed", "status": "completed" }
                ]
            }
        }"#,
        Arc::clone(&sink) as Arc<dyn CommsLogSink>,
    )
    .await;

    let handle = adapter
        .launch(launch_request(SESSION_ID))
        .await
        .expect("launch");
    let mut stream = adapter.events(&handle);
    adapter
        .send(
            &handle,
            SendRequest {
                text: "trigger the account update".to_owned(),
            },
        )
        .await
        .expect("send");
    // The account frame is drained on its own task, so wait for the neutral
    // event rather than for turn end (which it may well follow).
    collect_until(&mut stream, |event| {
        matches!(event, AgentEvent::RateLimitsUpdated { .. })
    })
    .await;

    assert!(
        has_frame(
            &sink.entries_for(SESSION_ID),
            CommsDirection::FromAgent,
            CommsFrameKind::Notification,
            Some("account/rateLimits/updated"),
        ),
        "the account frame is visible in the session's inspector: {:?}",
        sink.entries_for(SESSION_ID)
    );
}

/// Two sessions on ONE shared app-server keep separate logs: the second
/// session's frames never appear under the first's id.
#[tokio::test(flavor = "multi_thread")]
async fn a_second_session_on_the_same_server_does_not_leak_into_the_first() {
    let sink = RecordingSink::arc();
    // A scenario with no turn: launching twice is enough, and each launch is a
    // `thread/start` pair that must land under its own session.
    let (adapter, _guard) = adapter_with(
        r#"{ "thread_id": "thr_shared" }"#,
        Arc::clone(&sink) as Arc<dyn CommsLogSink>,
    )
    .await;

    adapter
        .launch(launch_request("sess-first"))
        .await
        .expect("first launch");
    let first_count = sink.entries_for("sess-first").len();
    adapter
        .launch(launch_request("sess-second"))
        .await
        .expect("second launch");

    assert_eq!(
        sink.entries_for("sess-first").len(),
        first_count,
        "the second session's launch added nothing to the first session's log"
    );
    assert!(
        !sink.entries_for("sess-second").is_empty(),
        "the second session has its own frames"
    );
}

/// Closing a session releases its log, so the buffers do not accumulate one per
/// closed session for the process's lifetime.
#[tokio::test(flavor = "multi_thread")]
async fn closing_a_session_discards_its_log() {
    let sink = RecordingSink::arc();
    let (adapter, _guard) = adapter_with(
        r#"{ "thread_id": "thr_close" }"#,
        Arc::clone(&sink) as Arc<dyn CommsLogSink>,
    )
    .await;

    let handle = adapter
        .launch(launch_request(SESSION_ID))
        .await
        .expect("launch");
    assert!(sink.discarded().is_empty(), "nothing discarded while open");

    adapter.close(&handle).await.expect("close");
    assert_eq!(sink.discarded(), vec![SESSION_ID.to_owned()]);
}

// --- Never blocks -----------------------------------------------------------

/// A full turn completes with **no** consumer attached to the log and with the
/// real hub's live channel saturated by a subscriber that never reads.
///
/// This is the load-bearing test of the whole feature: the inspector exists
/// because a headless session must never hang invisibly, so an inspector that is
/// not being read must not be able to make one hang. Driven through the real
/// [`CommsLogHub`] rather than a hand-written stub, because the non-blocking
/// property lives in the hub's bounded ring + lossy broadcast, not in the port.
#[tokio::test(flavor = "multi_thread")]
async fn a_turn_completes_with_a_saturated_log_and_no_consumer() {
    let hub = Arc::new(CommsLogHub::new());
    let (adapter, _guard) = adapter_with(
        approval_turn_scenario(),
        Arc::clone(&hub) as Arc<dyn CommsLogSink>,
    )
    .await;

    // A subscriber that never reads a single frame: its broadcast slot fills and
    // stays full for the whole turn.
    let _stalled = hub.subscribe(SESSION_ID);
    // Saturate it before the turn even starts, so the turn runs entirely against
    // a full channel rather than filling it as it goes.
    for i in 0..(delta_server::COMMS_RING_CAPACITY * 2) {
        hub.record(
            SESSION_ID,
            CommsEntry::new(
                CommsDirection::ToAgent,
                CommsFrameKind::Notification,
                Some("test/filler"),
                format!(r#"{{"n":{i}}}"#),
            ),
        );
    }

    let handle = adapter
        .launch(launch_request(SESSION_ID))
        .await
        .expect("launch");
    let mut stream = adapter.events(&handle);
    adapter
        .send(
            &handle,
            SendRequest {
                text: "run it".to_owned(),
            },
        )
        .await
        .expect("send");
    let until_permission = collect_until(&mut stream, |event| {
        matches!(event, AgentEvent::PermissionRequested { .. })
    })
    .await;
    let AgentEvent::PermissionRequested { request } =
        until_permission.last().expect("at least one event").clone()
    else {
        panic!("expected a permission request, got {until_permission:?}");
    };
    adapter
        .resolve_permission(&handle, &request.request_id, PermissionDecision::Allow)
        .await
        .expect("resolve permission");

    // The payoff: the turn reached completion. `collect_until` panics on a hang,
    // so arriving here IS the proof the sink never stalled the adapter.
    let events = collect_until(&mut stream, is_turn_completed).await;
    assert!(
        events.iter().any(is_turn_completed),
        "the turn completed with a saturated, unread log: {events:?}"
    );
}
