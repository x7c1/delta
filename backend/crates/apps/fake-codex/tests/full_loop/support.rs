//! The fixtures every loop in this suite shares: the scenario file on disk, the
//! real backend assembled over it, the REST request helpers, and the broadcast
//! drains.

use std::sync::Arc;
use std::time::Duration;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use axum::Router;
use serde_json::{json, Value};
use tower::ServiceExt;

use codex_agent::{CodexAdapterFactory, CodexLaunchConfig};
use delta_server::{router, AppState, CommsLogHub};
use delta_sqlite::SqliteStore;
use delta_transcript::JsonlTranscript;
use delta_usecase::{
    AgentAdapterFactory, CommsLogSink, GitWorktree, Interactor, SessionEvent, TmuxDriver,
    Transcript, Workspace,
};
use git_worktree::Git;
use tmux_driver::Tmux;
use workspace_fs::FsWorkspace;

/// The assistant reply the scripted turn completes with — the text the test
/// asserts was both streamed and persisted.
pub(crate) const REPLY: &str = "Hello from Codex";
/// The streaming fragment the scripted turn emits before the completed message,
/// a strict prefix of [`REPLY`] so it reads as a partial delta of the same reply.
pub(crate) const REPLY_FRAGMENT: &str = "Hello";
/// A short bound so a wiring bug fails the test fast instead of hanging it.
pub(crate) const TIMEOUT: Duration = Duration::from_secs(20);

/// A scenario file in a unique temp dir, removed on drop. The child `fake-codex`
/// is pointed at it via `FAKE_CODEX_SCENARIO` in the adapter's child env, so the
/// parent process's (shared) environment is never mutated.
pub(crate) struct ScenarioGuard {
    dir: std::path::PathBuf,
    path: std::path::PathBuf,
    /// Where the fake appends each `thread/inject_items` payload (one JSON line
    /// per call), handed to the child via `FAKE_CODEX_INJECT_LOG`. Empty/absent
    /// unless a turn injects hidden context — the branch loop reads it back.
    inject_log: std::path::PathBuf,
    /// Where the fake appends each `thread/start` params object (one JSON line
    /// per call), handed to the child via `FAKE_CODEX_THREAD_START_LOG`. The
    /// launch-options loop reads it back to prove the session's selection
    /// arrived as real `ThreadStartParams` fields.
    thread_start_log: std::path::PathBuf,
}

impl ScenarioGuard {
    pub(crate) fn write(scenario: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "fake-codex-full-loop-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("scenario.json");
        std::fs::write(&path, scenario).unwrap();
        let inject_log = dir.join("inject.log");
        let thread_start_log = dir.join("thread-start.log");
        Self {
            dir,
            path,
            inject_log,
            thread_start_log,
        }
    }

    /// Replace the scenario on disk, so the *next* `fake-codex` this guard's
    /// config spawns plays a different script.
    ///
    /// The adapter factory is built once with one scenario path, and each Codex
    /// session spawns its own `fake-codex` reading that path at startup — so
    /// rewriting the file between processes is how one backend drives a first
    /// process that dies and a second (the resume) that behaves. Already-running
    /// fakes are unaffected: they parsed the file at launch.
    pub(crate) fn rewrite(&self, scenario: &str) {
        std::fs::write(&self.path, scenario).unwrap();
    }

    /// The `thread/inject_items` payloads the fake recorded, one per line,
    /// parsed as JSON. Empty when the fake was never asked to inject (the file
    /// is created lazily on the first injection).
    pub(crate) fn injected_items(&self) -> Vec<Value> {
        read_json_lines(&self.inject_log)
    }

    /// The `thread/start` params the fake recorded, one per line, parsed as
    /// JSON. Empty when no thread was ever started.
    pub(crate) fn thread_starts(&self) -> Vec<Value> {
        read_json_lines(&self.thread_start_log)
    }

    /// An on-disk database path inside this guard's temp dir, for a test that
    /// needs a SECOND store handle over the same rows (an in-memory store cannot
    /// be shared: its connection owns the whole database).
    pub(crate) fn db_path(&self) -> String {
        self.dir.join("delta.db").to_string_lossy().into_owned()
    }
}

/// Read a sidecar record the fake keeps (one JSON value per line, in call
/// order). A missing file means the fake never wrote that record — the files
/// are created lazily on first use — so it reads as empty.
fn read_json_lines(path: &std::path::Path) -> Vec<Value> {
    match std::fs::read_to_string(path) {
        Ok(contents) => contents
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str(line).expect("recorded line is JSON"))
            .collect(),
        Err(_) => Vec::new(),
    }
}

impl Drop for ScenarioGuard {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).ok();
    }
}

/// Assemble the real backend wired to drive the `fake-codex` binary with
/// `scenario`, returning the router and the shared state (whose broadcast the
/// test subscribes to). Uses a fresh in-memory store.
pub(crate) fn build_app(scenario: &ScenarioGuard) -> (Router, AppState) {
    build_app_with(SqliteStore::open_in_memory().unwrap(), scenario)
}

/// Like [`build_app`] but over a caller-provided store, so a test can point two
/// separate backends (with distinct scenarios) at ONE on-disk database — the
/// server-restart simulation: the second backend boots with no in-process
/// bindings but the first's persisted rows + provider ids.
pub(crate) fn build_app_with(store: SqliteStore, scenario: &ScenarioGuard) -> (Router, AppState) {
    let codex_config = CodexLaunchConfig {
        // The fake IS the app-server, so it takes no `app-server` argument.
        codex_bin: env!("CARGO_BIN_EXE_fake-codex").to_owned(),
        args: vec![],
        // Hand the fake its scenario and sidecar record paths through the
        // child's env, not the parent's.
        env: vec![
            (
                "FAKE_CODEX_SCENARIO".to_owned(),
                scenario.path.to_string_lossy().into_owned(),
            ),
            (
                "FAKE_CODEX_INJECT_LOG".to_owned(),
                scenario.inject_log.to_string_lossy().into_owned(),
            ),
            (
                "FAKE_CODEX_THREAD_START_LOG".to_owned(),
                scenario.thread_start_log.to_string_lossy().into_owned(),
            ),
        ],
    };
    // The comms log is wired exactly as the composition root wires it: ONE hub
    // handed to the adapter factory (which records into it) and to the state
    // (which serves `/comms` from it). Sharing the instance is the whole point —
    // two hubs would look idle while frames piled up in the other.
    let comms_log = Arc::new(CommsLogHub::new());
    let factory: Arc<dyn AgentAdapterFactory> = Arc::new(
        CodexAdapterFactory::new(codex_config)
            .with_comms_log(Arc::clone(&comms_log) as Arc<dyn CommsLogSink>),
    );

    let interactor = Interactor::new(
        Box::new(Tmux::new("delta-codex-full-loop")) as Box<dyn TmuxDriver>,
        Box::new(JsonlTranscript::new()) as Box<dyn Transcript>,
        Box::new(store) as Box<dyn delta_usecase::SessionStore>,
        Box::new(FsWorkspace::new()) as Box<dyn Workspace>,
        Box::new(Git::new()) as Box<dyn GitWorktree>,
        std::env::temp_dir()
            .join("delta-codex-full-loop-session")
            .to_string_lossy()
            .into_owned(),
        std::env::temp_dir()
            .join("delta-codex-full-loop-worktrees")
            .to_string_lossy()
            .into_owned(),
        "{}",
        "/tmp/delta-codex-full-loop-settings.json",
    )
    .with_adapter_factory(factory);

    let state =
        AppState::from_interactor(interactor, "delta-codex-full-loop").with_comms_log(comms_log);
    (router(state.clone()), state)
}

pub(crate) async fn post_json(app: &Router, uri: &str, body: Value) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    json_response(response).await
}

pub(crate) async fn get(app: &Router, uri: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    json_response(response).await
}

async fn json_response(response: axum::response::Response) -> (StatusCode, Value) {
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, value)
}

/// A scenario whose `turn/start` plays one streamed assistant reply then
/// completes — the same shape `streaming`'s first test scripts inline. The fake
/// replays it on **every** `turn/start`, so it drives both the opening turn and
/// every subsequent one, which is exactly what the multi-turn test needs.
pub(crate) fn streaming_turn_scenario() -> ScenarioGuard {
    ScenarioGuard::write(&format!(
        r#"{{
            "thread_id": "thr_multi_turn",
            "turn": {{
                "turn_id": "turn_multi",
                "emit": [
                    {{ "type": "turn_started" }},
                    {{ "type": "item_started",   "item": {{ "id": "item_1", "type": "agentMessage" }} }},
                    {{ "type": "agent_message_delta", "item_id": "item_1", "delta": "{REPLY_FRAGMENT}" }},
                    {{ "type": "item_completed", "item": {{ "id": "item_1", "type": "agentMessage", "text": "{REPLY}" }} }},
                    {{ "type": "turn_completed", "status": "completed" }}
                ]
            }}
        }}"#
    ))
}

/// Drain the broadcast until the named session's turn completes, accumulating its
/// streamed assistant deltas and returning the streamed text. Fails the test on
/// timeout rather than hanging.
pub(crate) async fn drain_one_turn(
    events: &mut tokio::sync::broadcast::Receiver<SessionEvent>,
    session_id: &str,
) -> String {
    let mut streamed = String::new();
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    loop {
        let event = tokio::time::timeout_at(deadline, events.recv())
            .await
            .expect("timed out waiting for the Codex turn to complete over the broadcast")
            .expect("the broadcast channel stayed open");
        match event {
            SessionEvent::AssistantStreaming {
                session_id: sid,
                delta,
                ..
            } if sid.as_str() == session_id => streamed.push_str(&delta),
            SessionEvent::TurnCompleted {
                session_id: sid, ..
            } if sid.as_str() == session_id => return streamed,
            _ => {}
        }
    }
}

/// Register one launch option over the REST registry and return its id.
pub(crate) async fn register_launch_option(
    app: &Router,
    name: &str,
    value: Option<&str>,
    provider: &str,
) -> i64 {
    let mut body = json!({ "name": name, "provider": provider });
    if let Some(value) = value {
        body["value"] = json!(value);
    }
    let (status, created) = post_json(app, "/api/launch-options", body).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "the launch option was registered: {created:?}"
    );
    created["id"]
        .as_i64()
        .expect("the created launch option carries its id")
}

/// Pump the broadcast until the turn completes, so a test can line up against a
/// finished turn without re-implementing the wait.
pub(crate) async fn await_turn_completion(
    events: &mut tokio::sync::broadcast::Receiver<SessionEvent>,
) {
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    loop {
        let event = tokio::time::timeout_at(deadline, events.recv())
            .await
            .expect("timed out waiting for the turn to complete")
            .expect("the broadcast channel stayed open");
        if matches!(event, SessionEvent::TurnCompleted { .. }) {
            return;
        }
    }
}
