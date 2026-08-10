//! The Codex full-loop end-to-end test: browser → server → `fake-codex`.
//!
//! This is the payoff proof for the live Codex event pump. It assembles the
//! **real** backend — `delta_server::router` over an `AppState` wired with the
//! real gateways (an in-memory `SqliteStore`, the real transcript/tmux/workspace
//! gateways, all inert on the terminal-less Codex path) plus a real
//! `CodexAdapterFactory` pointing at the compiled `fake-codex` binary — and
//! drives one turn through the whole stack:
//!
//! 1. `POST /api/sends` with `provider: "codex"` and a first prompt. Composition
//!    dispatches this to the terminal-less Codex path, which stands up the
//!    `fake-codex` subprocess (spawn + `initialize` handshake), starts a thread
//!    (`thread/start`), and starts the turn (`turn/start`).
//! 2. The fake plays a scripted turn: a streaming assistant fragment
//!    (`item/started` with text), the completed assistant message
//!    (`item/completed`), then `turn/completed`.
//! 3. The event pump drains the adapter's event stream and drives the session
//!    actor, which persists the completed message and emits the browser events on
//!    the async seam — reaching the broadcast the WebSocket forwards.
//!
//! The test asserts, over that broadcast, that the assistant message was
//! **streamed** (`AssistantStreaming`) and its turn **completed**
//! (`TurnCompleted`), and, over `GET /api/threads/{id}/messages`, that the
//! assistant message was **persisted** — the create → prompt → assistant →
//! turn-complete loop, offline and deterministic.
//!
//! The sibling loops in this file cover the other browser-driven turn controls
//! over the same real stack: the permission round-trip (approve/deny) and the
//! interrupt of an in-flight turn.

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
const REPLY: &str = "Hello from Codex";
/// The streaming fragment the scripted turn emits before the completed message,
/// a strict prefix of [`REPLY`] so it reads as a partial delta of the same reply.
const REPLY_FRAGMENT: &str = "Hello";
/// A short bound so a wiring bug fails the test fast instead of hanging it.
const TIMEOUT: Duration = Duration::from_secs(20);

/// A scenario file in a unique temp dir, removed on drop. The child `fake-codex`
/// is pointed at it via `FAKE_CODEX_SCENARIO` in the adapter's child env, so the
/// parent process's (shared) environment is never mutated.
struct ScenarioGuard {
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
    fn write(scenario: &str) -> Self {
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

    /// The `thread/inject_items` payloads the fake recorded, one per line,
    /// parsed as JSON. Empty when the fake was never asked to inject (the file
    /// is created lazily on the first injection).
    fn injected_items(&self) -> Vec<Value> {
        read_json_lines(&self.inject_log)
    }

    /// The `thread/start` params the fake recorded, one per line, parsed as
    /// JSON. Empty when no thread was ever started.
    fn thread_starts(&self) -> Vec<Value> {
        read_json_lines(&self.thread_start_log)
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
fn build_app(scenario: &ScenarioGuard) -> (Router, AppState) {
    build_app_with(SqliteStore::open_in_memory().unwrap(), scenario)
}

/// Like [`build_app`] but over a caller-provided store, so a test can point two
/// separate backends (with distinct scenarios) at ONE on-disk database — the
/// server-restart simulation: the second backend boots with no in-process
/// bindings but the first's persisted rows + provider ids.
fn build_app_with(store: SqliteStore, scenario: &ScenarioGuard) -> (Router, AppState) {
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

async fn post_json(app: &Router, uri: &str, body: Value) -> (StatusCode, Value) {
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

async fn get(app: &Router, uri: &str) -> (StatusCode, Value) {
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

#[tokio::test(flavor = "multi_thread")]
async fn codex_prompt_streams_persists_and_completes_over_the_full_stack() {
    // A scripted turn using the real item shapes: the assistant item starts
    // (announced, no text yet), a streaming `item/agentMessage/delta` carries a
    // strict prefix of the reply (→ a live `AssistantDelta`), the completed item
    // carries the full text (→ the persisted `AssistantMessage`), then a clean
    // turn completion.
    let scenario = ScenarioGuard::write(&format!(
        r#"{{
            "thread_id": "thr_full_loop",
            "turn": {{
                "turn_id": "turn_full_loop",
                "emit": [
                    {{ "type": "turn_started" }},
                    {{ "type": "item_started",   "item": {{ "id": "item_1", "type": "agentMessage" }} }},
                    {{ "type": "agent_message_delta", "item_id": "item_1", "delta": "{REPLY_FRAGMENT}" }},
                    {{ "type": "item_completed", "item": {{ "id": "item_1", "type": "agentMessage", "text": "{REPLY}" }} }},
                    {{ "type": "turn_completed", "status": "completed" }}
                ]
            }}
        }}"#
    ));

    let (app, state) = build_app(&scenario);
    // Subscribe and start the async-seam drain BEFORE the prompt, so no event the
    // pump emits after the send returns can be missed.
    let mut events = state.subscribe();
    state
        .spawn_async_event_drain()
        .expect("the async drain is taken exactly once");

    // 1. Create a Codex session with a first prompt over the REST surface.
    let (status, body) = post_json(
        &app,
        "/api/sends",
        json!({ "new_session": true, "provider": "codex", "text": "hello codex" }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "the send was created: {body:?}"
    );
    let session_id = body["send"]["session_id"]
        .as_str()
        .expect("the send response carries its session id")
        .to_owned();
    let thread_id = body["send"]["thread_id"]
        .as_i64()
        .expect("the send response carries its main thread id");

    // 2. Collect the pump's broadcast events until the turn is proven streamed
    //    and completed (or the timeout trips).
    let mut streamed_reply = String::new();
    let mut turn_completed = false;
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    while !turn_completed {
        let event = tokio::time::timeout_at(deadline, events.recv())
            .await
            .expect("timed out waiting for the Codex turn to complete over the broadcast")
            .expect("the broadcast channel stayed open");
        match event {
            SessionEvent::AssistantStreaming {
                session_id: sid,
                delta,
                final_,
                ..
            } => {
                assert_eq!(sid.as_str(), session_id, "streaming names our session");
                assert!(!final_, "a Codex streaming delta is never the final chunk");
                streamed_reply.push_str(&delta);
            }
            SessionEvent::TurnCompleted {
                session_id: sid,
                thread_id: tid,
                ..
            } => {
                assert_eq!(
                    sid.as_str(),
                    session_id,
                    "turn completion names our session"
                );
                assert_eq!(
                    tid,
                    Some(delta_usecase::ThreadId(thread_id)),
                    "the completed turn is attributed to the session's main thread"
                );
                turn_completed = true;
            }
            _ => {}
        }
    }
    assert_eq!(
        streamed_reply, REPLY_FRAGMENT,
        "the assistant reply was streamed live before the turn completed"
    );

    // 3. The completed assistant message was persisted: the store-backed
    //    messages endpoint (not the live event) returns it on the main thread.
    let (status, body) = get(&app, &format!("/api/threads/{thread_id}/messages")).await;
    assert_eq!(status, StatusCode::OK, "messages fetched: {body:?}");
    let messages = body["messages"]
        .as_array()
        .expect("the messages response carries a messages array");
    let assistant = messages
        .iter()
        .find(|m| m["role"] == json!("assistant"))
        .expect("the assistant message was persisted");
    assert_eq!(
        assistant["content_text"],
        json!(REPLY),
        "the persisted assistant message carries the completed reply"
    );
    assert_eq!(
        assistant["provider_item_id"],
        json!("item_1"),
        "the persisted message keeps the provider item id as its reconcile key"
    );
    // The message time reached the persisted row: the item's `completedAtMs`
    // envelope was carried onto the neutral event and folded into `created_at`
    // as the canonical ISO-8601 UTC string (converted from `ENVELOPE_TS_MS`).
    assert_eq!(
        assistant["created_at"],
        json!("2026-07-17T07:12:18.000Z"),
        "the Codex item timestamp is persisted as an ISO-8601 created_at"
    );
    // This scenario's app-server reports no git metadata at all — the shape a
    // thread outside a git working tree gets — so the branch degrades to null
    // rather than being invented, all the way through to the persisted row.
    assert_eq!(
        assistant["git_branch"],
        Value::Null,
        "no gitInfo in the thread/start response means no branch on the message"
    );
    // The user prompt persisted too, so the loop is a real conversation.
    let user = messages
        .iter()
        .find(|m| m["role"] == json!("user"))
        .expect("the user prompt was persisted as well");
    assert!(
        user["created_at"].as_str().is_some_and(|s| !s.is_empty()),
        "the persisted user prompt also carries a non-empty created_at, got {:?}",
        user["created_at"]
    );
}

/// The Codex **reasoning** full loop: a turn whose model reasons before replying
/// must persist that reasoning as a `thinking` content block, so a Codex session
/// shows the model's thinking exactly as a Claude one does.
///
/// The scripted turn plays the real reasoning shapes: the `reasoning` item opens
/// empty, streams a summary fragment (`item/reasoning/summaryTextDelta`), and
/// completes with its `summary` parts; the assistant reply follows as its own
/// `agentMessage` item. The test asserts the reasoning landed as its own
/// `thinking` block on its own message AND — the invariant the earlier drop
/// existed to protect — that it was never mis-filed as reply text, neither in the
/// persisted assistant message nor in the live `AssistantStreaming` preview.
#[tokio::test(flavor = "multi_thread")]
async fn codex_reasoning_persists_as_a_thinking_block_and_is_never_reply_text() {
    let scenario = ScenarioGuard::write(&format!(
        r#"{{
            "thread_id": "thr_reasoning",
            "turn": {{
                "turn_id": "turn_reasoning",
                "emit": [
                    {{ "type": "turn_started" }},
                    {{ "type": "item_started",   "item": {{ "id": "reason_1", "type": "reasoning", "content": [], "summary": [] }} }},
                    {{ "type": "notification", "method": "item/reasoning/summaryTextDelta",
                       "params": {{ "itemId": "reason_1", "summaryIndex": 0, "delta": "Weighing" }} }},
                    {{ "type": "item_completed", "item": {{ "id": "reason_1", "type": "reasoning", "content": [],
                       "summary": ["Weighing the options.", "Picking the simplest."] }} }},
                    {{ "type": "item_started",   "item": {{ "id": "item_1", "type": "agentMessage" }} }},
                    {{ "type": "agent_message_delta", "item_id": "item_1", "delta": "{REPLY_FRAGMENT}" }},
                    {{ "type": "item_completed", "item": {{ "id": "item_1", "type": "agentMessage", "text": "{REPLY}" }} }},
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

    let (status, body) = post_json(
        &app,
        "/api/sends",
        json!({ "new_session": true, "provider": "codex", "text": "think it through" }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "the send was created: {body:?}"
    );
    let session_id = body["send"]["session_id"]
        .as_str()
        .expect("the send response carries its session id")
        .to_owned();
    let thread_id = body["send"]["thread_id"]
        .as_i64()
        .expect("the send response carries its main thread id");

    let streamed_reply = drain_one_turn(&mut events, &session_id).await;
    assert_eq!(
        streamed_reply, REPLY_FRAGMENT,
        "only the reply streams live; the reasoning fragment must not reach the \
         assistant preview, which would show the model's thinking as its answer"
    );

    let (status, body) = get(&app, &format!("/api/threads/{thread_id}/messages")).await;
    assert_eq!(status, StatusCode::OK, "messages fetched: {body:?}");
    let messages = body["messages"]
        .as_array()
        .expect("the messages response carries a messages array");

    // The reasoning persisted as its own message, carrying a single `thinking`
    // block whose parts joined into one text.
    let reasoning = messages
        .iter()
        .find(|m| m["provider_item_id"] == json!("reason_1"))
        .expect("the reasoning item was persisted");
    assert_eq!(reasoning["role"], json!("assistant"));
    assert_eq!(
        reasoning["content"],
        json!([{
            "type": "thinking",
            "thinking": "Weighing the options.\n\nPicking the simplest.",
        }]),
        "the reasoning is a thinking block, not a text block"
    );

    // The reply is a separate message and carries only the reply — the reasoning
    // never leaked into it.
    let reply = messages
        .iter()
        .find(|m| m["provider_item_id"] == json!("item_1"))
        .expect("the assistant reply was persisted");
    assert_eq!(
        reply["content"],
        json!([{ "type": "text", "text": REPLY }]),
        "the reply carries only its own text"
    );
    assert!(
        !messages.iter().any(|m| {
            m["content"].as_array().is_some_and(|blocks| {
                blocks.iter().any(|b| {
                    b["type"] == json!("text")
                        && b["text"].as_str().is_some_and(|t| t.contains("Weighing"))
                })
            })
        }),
        "no persisted text block may carry the reasoning: {messages:?}"
    );
}

/// A scenario whose `turn/start` plays one streamed assistant reply then
/// completes — the same shape the first full-loop test uses. The fake replays it
/// on **every** `turn/start`, so it drives both the opening turn and every
/// subsequent one, which is exactly what the multi-turn test needs.
fn streaming_turn_scenario() -> ScenarioGuard {
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

/// A two-turn scenario for the branch loop: the opening turn and the branch turn
/// carry DISTINCT turn/item ids (played from the `turns` sequence, one per
/// `turn/start`), mirroring a real `codex app-server`. This is what lets the
/// branch turn's persisted messages be told apart from the opening turn's — the
/// single-turn [`streaming_turn_scenario`] reuses one id set across turns, so
/// its rows would reconcile onto each other by uuid and the per-thread routing
/// could not be observed.
fn branching_turns_scenario() -> ScenarioGuard {
    let turn = |turn_id: &str, item_id: &str| {
        format!(
            r#"{{
                "turn_id": "{turn_id}",
                "emit": [
                    {{ "type": "turn_started" }},
                    {{ "type": "item_started",   "item": {{ "id": "{item_id}", "type": "agentMessage" }} }},
                    {{ "type": "agent_message_delta", "item_id": "{item_id}", "delta": "{REPLY_FRAGMENT}" }},
                    {{ "type": "item_completed", "item": {{ "id": "{item_id}", "type": "agentMessage", "text": "{REPLY}" }} }},
                    {{ "type": "turn_completed", "status": "completed" }}
                ]
            }}"#
        )
    };
    ScenarioGuard::write(&format!(
        r#"{{
            "thread_id": "thr_branch_loop",
            "turns": [ {open}, {branch} ]
        }}"#,
        open = turn("turn_open", "item_open"),
        branch = turn("turn_branch", "item_branch"),
    ))
}

/// Drain the broadcast until the named session's turn completes, accumulating its
/// streamed assistant deltas and returning the streamed text. Fails the test on
/// timeout rather than hanging.
async fn drain_one_turn(
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
async fn register_launch_option(
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

/// The Codex **launch-options** full loop: options the user registered for
/// Codex and selected when starting a session reach the provider as
/// `thread/start` fields.
///
/// This is the regression proof for the bug where the Settings UI happily
/// registered a Codex-scoped launch option and the new-session picker offered
/// it, but selecting it made the spawn fail outright — the core rejected any
/// selection for a non-Claude provider. The test registers three options over
/// the real REST registry, starts a Codex session selecting them, and reads
/// back the `thread/start` params the fake app-server actually received.
///
/// It also pins the value-mapping rule: a value that is not valid JSON is the
/// string it looks like, a value that parses keeps its real type, and a
/// valueless option is the bare boolean `true`.
#[tokio::test(flavor = "multi_thread")]
async fn codex_launch_options_reach_thread_start_over_the_full_stack() {
    let scenario = streaming_turn_scenario();
    let (app, state) = build_app(&scenario);
    let mut events = state.subscribe();
    state
        .spawn_async_event_drain()
        .expect("the async drain is taken exactly once");

    // A plain string value, a JSON-object value, and a valueless option.
    let model = register_launch_option(&app, "model", Some("gpt-5.6-sol"), "codex").await;
    let config = register_launch_option(
        &app,
        "config",
        Some(r#"{"tools":{"web_search":true}}"#),
        "codex",
    )
    .await;
    let ephemeral = register_launch_option(&app, "ephemeral", None, "codex").await;

    let (status, body) = post_json(
        &app,
        "/api/sends",
        json!({
            "new_session": true,
            "provider": "codex",
            "text": "hello codex",
            "launch_option_ids": [model, config, ephemeral],
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "a Codex session selecting launch options starts (it used to fail): {body:?}"
    );
    let session_id = body["send"]["session_id"]
        .as_str()
        .expect("the send response carries its session id")
        .to_owned();

    // Let the opening turn finish, so the session is unambiguously live rather
    // than merely accepted.
    drain_one_turn(&mut events, &session_id).await;

    let starts = scenario.thread_starts();
    assert_eq!(starts.len(), 1, "one thread was started: {starts:?}");
    let params = &starts[0];
    assert!(
        params["cwd"].as_str().is_some_and(|cwd| !cwd.is_empty()),
        "Delta's own cwd is still sent, got {params:?}"
    );
    assert_eq!(
        params["model"],
        json!("gpt-5.6-sol"),
        "a non-JSON value arrives as the string it looks like"
    );
    assert_eq!(
        params["config"],
        json!({ "tools": { "web_search": true } }),
        "a JSON value arrives with its real type, not as a quoted string"
    );
    assert_eq!(
        params["ephemeral"],
        json!(true),
        "a valueless option switches its boolean field on"
    );
}

/// The Codex **message-metadata** full loop: a persisted Codex message reports
/// the model the server resolved for the thread, the branch the server observed,
/// and the directory the session is running in — the feedback channel for a
/// user-selectable model.
///
/// The session selects `model=requested-by-delta` as a launch option while the
/// fake app-server answers `thread/start` with a *different* top-level `model`.
/// That divergence is the whole point: Delta's request is only one input to the
/// server's decision (the user's own `config.toml` and the server's default are
/// others), so only the response says what is actually running. Asserting the
/// **server's** value proves the metadata is read back rather than echoed.
///
/// The branch is exercised over a **real git repository** created for this test,
/// with the session started in it and **no worktree**. That combination is the
/// case the feature exists for: Delta fills the session row's `branch_at_launch`
/// only on the worktree path, and Codex's `thread/start` reports no git metadata
/// at all, so a branch on these messages can only come from Delta observing its
/// launch directory. Using a real repo (not a scripted fake) means the real
/// `Git` gateway runs, so the value is one `git` itself produced.
///
/// `cwd` is checked against the `cwd` Delta itself sent on `thread/start`, so the
/// message reports the same launch directory the agent was started in — not a
/// separately re-derived path that could drift from it.
#[tokio::test(flavor = "multi_thread")]
async fn codex_messages_report_the_resolved_model_the_observed_branch_and_the_launch_dir() {
    const RESOLVED_MODEL: &str = "gpt-5.6-sol";
    const OBSERVED_BRANCH: &str = "feature/observed-by-delta";
    let repo = GitRepoGuard::init(OBSERVED_BRANCH);
    let scenario = ScenarioGuard::write(&format!(
        r#"{{
            "thread_id": "thr_metadata",
            "model": "{RESOLVED_MODEL}",
            "turn": {{
                "turn_id": "turn_metadata",
                "emit": [
                    {{ "type": "turn_started" }},
                    {{ "type": "item_started",   "item": {{ "id": "item_1", "type": "agentMessage" }} }},
                    {{ "type": "item_completed", "item": {{ "id": "item_1", "type": "agentMessage", "text": "{REPLY}" }} }},
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

    let model_option =
        register_launch_option(&app, "model", Some("requested-by-delta"), "codex").await;
    let (status, body) = post_json(
        &app,
        "/api/sends",
        json!({
            "new_session": true,
            "provider": "codex",
            "text": "hello codex",
            "launch_option_ids": [model_option],
            // A real git repo, and NO worktree: the case Delta records no
            // branch_at_launch for.
            "workdir": repo.path(),
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "the send was created: {body:?}"
    );
    let session_id = body["send"]["session_id"].as_str().unwrap().to_owned();
    let thread_id = body["send"]["thread_id"].as_i64().unwrap();

    drain_one_turn(&mut events, &session_id).await;

    // What Delta asked for, and where it launched, as the fake actually received
    // them.
    let starts = scenario.thread_starts();
    assert_eq!(starts.len(), 1, "one thread was started: {starts:?}");
    assert_eq!(
        starts[0]["model"],
        json!("requested-by-delta"),
        "the selected launch option did ride the request"
    );
    let launch_cwd = starts[0]["cwd"]
        .as_str()
        .expect("Delta always sends its own cwd")
        .to_owned();

    let (status, body) = get(&app, &format!("/api/threads/{thread_id}/messages")).await;
    assert_eq!(status, StatusCode::OK, "messages fetched: {body:?}");
    let messages = body["messages"].as_array().unwrap();
    assert!(!messages.is_empty(), "the turn persisted messages");
    for message in messages {
        assert_eq!(
            message["model"],
            json!(RESOLVED_MODEL),
            "the persisted message reports the model the SERVER resolved, not the \
             `requested-by-delta` Delta asked for: {message:?}"
        );
        assert_eq!(
            message["cwd"],
            json!(launch_cwd),
            "the persisted message reports the directory the session launched in: {message:?}"
        );
        assert_eq!(
            message["git_branch"],
            json!(OBSERVED_BRANCH),
            "the persisted message reports the branch of its launch directory, \
             observed by Delta — this session has no worktree, so nothing else \
             knows it: {message:?}"
        );
    }
}

/// A throwaway git repository on a named branch, removed on drop.
///
/// Used by the metadata loop to exercise the real `Git` gateway: the branch a
/// message reports must be one `git` actually produced, not a scripted fake.
struct GitRepoGuard {
    dir: std::path::PathBuf,
}

impl GitRepoGuard {
    /// Create a repository with one empty commit on `branch`.
    ///
    /// The commit is required, not decorative: on an unborn HEAD
    /// `git rev-parse --abbrev-ref HEAD` exits non-zero, which the gateway
    /// (correctly) reads as "no branch". `--initial-branch` pins the name rather
    /// than depending on the host's `init.defaultBranch`, and the identity is
    /// passed with `-c` so the test does not depend on the host's git config
    /// either.
    fn init(branch: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "fake-codex-metadata-repo-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        Self::git(&dir, &["init", "--quiet", "--initial-branch", branch]);
        Self::git(
            &dir,
            &[
                "-c",
                "user.email=test@example.invalid",
                "-c",
                "user.name=delta test",
                "commit",
                "--quiet",
                "--allow-empty",
                "-m",
                "initial",
            ],
        );
        Self { dir }
    }

    /// Run one `git` invocation in `dir`, failing the test if it does not
    /// succeed — a silently broken fixture would look like a missing branch.
    fn git(dir: &std::path::Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .status()
            .expect("git must be available to run this test");
        assert!(status.success(), "git {args:?} failed in {dir:?}");
    }

    fn path(&self) -> String {
        self.dir.to_string_lossy().into_owned()
    }
}

impl Drop for GitRepoGuard {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).ok();
    }
}

/// A launch option naming a `thread/start` field Delta fills in itself is
/// rejected loudly at spawn: `400` naming the offending key, and no session row
/// left behind.
///
/// `cwd` is the field that matters — with a worktree it is the resolved
/// worktree path, and the session's repo-root / display-name /
/// branch-at-launch columns are recorded against it — so a user option
/// silently overriding it would leave those columns describing a directory the
/// agent is not running in. Failing the spawn is the only honest answer.
#[tokio::test(flavor = "multi_thread")]
async fn a_codex_launch_option_overriding_a_delta_owned_field_fails_the_spawn() {
    let scenario = streaming_turn_scenario();
    let (app, _state) = build_app(&scenario);

    let cwd = register_launch_option(&app, "cwd", Some("/somewhere/else"), "codex").await;

    let (status, body) = post_json(
        &app,
        "/api/sends",
        json!({
            "new_session": true,
            "provider": "codex",
            "text": "hello codex",
            "launch_option_ids": [cwd],
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "a Delta-owned field is rejected, not silently applied: {body:?}"
    );
    assert!(
        body["error"].as_str().is_some_and(|e| e.contains("cwd")),
        "the error names the offending key, got {body:?}"
    );

    // The eager session row was rolled back, so a rejected spawn leaves nothing
    // behind for the navigator to show.
    let (status, sessions) = get(&app, "/api/sessions").await;
    assert_eq!(status, StatusCode::OK, "sessions fetched: {sessions:?}");
    assert_eq!(
        sessions["sessions"].as_array().map(Vec::len),
        Some(0),
        "a rejected spawn leaves no session row: {sessions:?}"
    );
}

/// The Codex **multi-turn** full loop: a second (and later) message to an
/// existing Codex session must dispatch over the bound adapter, exactly like the
/// opening turn — not down Claude's pane/`--resume` path.
///
/// This is the regression proof for the dogfooding bug where every send after
/// the first failed: a subsequent send went through `enqueue_to_thread`, which
/// called `ensure_open()` → `open_session()` (`claude --resume`) and, for a
/// terminal-less Codex session (no pane, no transcript), returned
/// `ResumeUnavailable` — surfaced to the browser as a `409 CONFLICT` "cannot be
/// resumed" notice. The test creates a Codex session (turn 1), lets it complete,
/// then sends a SECOND message to the same thread and asserts the send is
/// accepted (`201 CREATED`, *not* the pre-fix `409`) and that the second turn
/// also starts, streams its reply, and completes over the same event pump.
///
/// Before the fix the second `POST /api/sends` returns `409 CONFLICT` and this
/// fails at the status assertion; after the fix it returns `201` and drives a
/// full second turn.
#[tokio::test(flavor = "multi_thread")]
async fn codex_second_message_dispatches_over_the_adapter_not_a_claude_resume() {
    let scenario = streaming_turn_scenario();
    let (app, state) = build_app(&scenario);
    // Subscribe and start the async-seam drain BEFORE the first prompt.
    let mut events = state.subscribe();
    state
        .spawn_async_event_drain()
        .expect("the async drain is taken exactly once");

    // Turn 1: create a Codex session with a first prompt.
    let (status, body) = post_json(
        &app,
        "/api/sends",
        json!({ "new_session": true, "provider": "codex", "text": "first message" }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "the first send was created: {body:?}"
    );
    let session_id = body["send"]["session_id"]
        .as_str()
        .expect("the send response carries its session id")
        .to_owned();
    let thread_id = body["send"]["thread_id"]
        .as_i64()
        .expect("the send response carries its main thread id");

    // Let turn 1 stream and complete.
    let streamed = drain_one_turn(&mut events, &session_id).await;
    assert_eq!(
        streamed, REPLY_FRAGMENT,
        "the first turn streamed its reply before completing"
    );

    // Turn 2: send a SECOND message to the SAME session's thread. This is the
    // send that failed before the fix.
    let (status, body) = post_json(
        &app,
        "/api/sends",
        json!({ "thread_id": thread_id, "text": "second message" }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "the second send dispatched over the adapter (no ResumeUnavailable/Claude-resume 409): {body:?}"
    );
    assert_eq!(
        body["send"]["session_id"].as_str().unwrap(),
        session_id,
        "the second send stays on the same session"
    );
    assert_eq!(
        body["send"]["thread_id"].as_i64().unwrap(),
        thread_id,
        "the second send is written against the same thread it targeted"
    );

    // Turn 2 also starts, streams, and completes over the already-running pump.
    let streamed = drain_one_turn(&mut events, &session_id).await;
    assert_eq!(
        streamed, REPLY_FRAGMENT,
        "the second turn streamed its reply live before completing"
    );

    // The session stayed open throughout — a single pump drove both turns, and
    // no resume path tore it down.
    let (status, body) = get(&app, "/api/sessions").await;
    assert_eq!(status, StatusCode::OK, "sessions listed: {body:?}");
    let session = body["sessions"]
        .as_array()
        .expect("the sessions response carries a sessions array")
        .iter()
        .find(|s| s["session"]["id"] == json!(session_id))
        .expect("our session is listed");
    assert_eq!(
        session["open"],
        json!(true),
        "the session stays open across both turns (one pump, no resume teardown)"
    );
}

/// The Codex **branch-from-selected-text** full loop: browser → server →
/// `fake-codex`.
///
/// This is the payoff proof for Codex branch send over `thread/inject_items`.
/// After an opening turn completes, the browser sends a branch send — a
/// `thread_id` send carrying `semantic_parent_uuid` (the branched-from message)
/// and `locator_quote` (the selected passage). The stack must:
///
/// 1. Accept it (`201 CREATED`) — NOT the old `ForkCapability::None` rejection.
/// 2. Deliver the selected passage to the fake as `thread/inject_items` (hidden
///    context), which the fake records to its inject log for this assertion.
/// 3. Create the same delta-side branch structure Claude builds — a NEW thread
///    lane parented to the source thread and rooted at the branched-from
///    message (visible over `GET /api/sessions/{id}/threads`).
/// 4. Dispatch the branch turn over the same Codex send path, so it streams and
///    completes over the running event pump.
#[tokio::test(flavor = "multi_thread")]
async fn codex_branch_from_selected_text_injects_context_and_completes_over_the_full_stack() {
    const QUOTE: &str = "the selected passage to branch from";
    const PARENT_UUID: &str = "msg-branch-parent";

    let scenario = branching_turns_scenario();
    let (app, state) = build_app(&scenario);
    let mut events = state.subscribe();
    state
        .spawn_async_event_drain()
        .expect("the async drain is taken exactly once");

    // Turn 1: create a Codex session with a first prompt, and let it complete so
    // the session is idle before the branch send.
    let (status, body) = post_json(
        &app,
        "/api/sends",
        json!({ "new_session": true, "provider": "codex", "text": "first message" }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "the first send was created: {body:?}"
    );
    let session_id = body["send"]["session_id"].as_str().unwrap().to_owned();
    let main_thread = body["send"]["thread_id"].as_i64().unwrap();
    drain_one_turn(&mut events, &session_id).await;

    // The branch send: same thread, plus the branched-from message and the
    // selected passage. Before the fix this returned an `Error::Agent`
    // rejection ("branching is not supported for a Codex session"); after it is
    // accepted and dispatches a branch turn.
    let (status, body) = post_json(
        &app,
        "/api/sends",
        json!({
            "thread_id": main_thread,
            "semantic_parent_uuid": PARENT_UUID,
            "locator_quote": QUOTE,
            "text": "branch text",
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "the Codex branch send is accepted (no ForkCapability rejection): {body:?}"
    );
    let branch_thread = body["send"]["thread_id"].as_i64().unwrap();
    assert_ne!(
        branch_thread, main_thread,
        "the branch send lands on a new thread lane, not the source thread"
    );
    assert_eq!(
        body["send"]["semantic_parent_uuid"].as_str(),
        Some(PARENT_UUID),
        "the branch send carries the branched-from message as its semantic parent"
    );
    assert_eq!(
        body["send"]["locator_quote"].as_str(),
        Some(QUOTE),
        "the branch send row persists the selected passage as its locator quote"
    );

    // (2) The fake received `thread/inject_items` with the selected passage as a
    // Responses API user message — the hidden context the model sees this turn.
    let injected = scenario.injected_items();
    assert_eq!(
        injected.len(),
        1,
        "exactly one thread/inject_items reached the fake, got {injected:?}"
    );
    let item = &injected[0][0];
    assert_eq!(
        item["type"],
        json!("message"),
        "the injected item is a message"
    );
    assert_eq!(item["role"], json!("user"), "injected as a user message");
    assert_eq!(
        item["content"][0]["type"],
        json!("input_text"),
        "the injected content is input_text"
    );
    assert_eq!(
        item["content"][0]["text"],
        json!(QUOTE),
        "the injected item carries the branched-from passage verbatim"
    );

    // (3) A new delta thread/branch exists with the right structure: parented to
    // the source thread, rooted at the branched-from message, titled from the
    // selected passage.
    let (status, body) = get(&app, &format!("/api/sessions/{session_id}/threads")).await;
    assert_eq!(status, StatusCode::OK, "threads listed: {body:?}");
    let child = body["threads"]
        .as_array()
        .expect("the threads response carries a threads array")
        .iter()
        .find(|t| t["id"].as_i64() == Some(branch_thread))
        .expect("the branch child thread is listed");
    assert_eq!(
        child["parent_thread_id"].as_i64(),
        Some(main_thread),
        "the branch child is parented to the source thread"
    );
    assert_eq!(
        child["root_message_uuid"].as_str(),
        Some(PARENT_UUID),
        "the branch child is rooted at the branched-from message"
    );
    assert_eq!(
        child["title"].as_str(),
        Some(QUOTE),
        "the branch child is titled provisionally from the selected passage"
    );

    // (4) The branch turn dispatched over the adapter: it streams and completes
    // over the same running event pump as any other Codex turn.
    let streamed = drain_one_turn(&mut events, &session_id).await;
    assert_eq!(
        streamed, REPLY_FRAGMENT,
        "the branch turn streamed its reply live before completing"
    );

    // (5) The regression this fix targets: the branch turn's persisted content
    // lands on the BRANCH thread, not main. From the live dev DB, the branch was
    // created and the `send` row routed correctly, yet `CodexConversationSource`
    // hardcoded the main thread + a null semantic parent, so the branch turn's
    // user prompt and assistant reply were written to main — leaving the branch
    // thread empty (the "no thread was created" symptom). These assertions fail
    // before the fix (the branch thread has no messages) and pass after it.
    let (status, body) = get(&app, &format!("/api/threads/{branch_thread}/messages")).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "branch thread messages fetched: {body:?}"
    );
    let branch_messages = body["messages"]
        .as_array()
        .expect("the branch thread's messages response carries a messages array");
    let branch_user = branch_messages
        .iter()
        .find(|m| m["role"] == json!("user"))
        .expect(
            "the branch turn's user prompt persisted ON THE BRANCH THREAD (empty before the fix)",
        );
    assert_eq!(
        branch_user["content_text"],
        json!("branch text"),
        "the branch user prompt carries the branch turn's text"
    );
    assert_eq!(
        branch_user["thread_id"].as_i64(),
        Some(branch_thread),
        "the branch user prompt is stored on the branch thread, not main"
    );
    assert_eq!(
        branch_user["semantic_parent_uuid"].as_str(),
        Some(PARENT_UUID),
        "the branch-ROOT user message carries the branched-from message as its \
         semantic parent, matching the send row"
    );
    let branch_assistant = branch_messages
        .iter()
        .find(|m| m["role"] == json!("assistant"))
        .expect("the branch turn's assistant reply persisted on the branch thread");
    assert_eq!(
        branch_assistant["content_text"],
        json!(REPLY),
        "the branch assistant reply persisted on the branch thread"
    );
    assert_eq!(
        branch_assistant["thread_id"].as_i64(),
        Some(branch_thread),
        "the branch assistant reply is stored on the branch thread"
    );
    assert!(
        branch_assistant["semantic_parent_uuid"].is_null(),
        "only the branch root carries the semantic parent, not the assistant reply"
    );

    // ...and the MAIN thread did NOT gain the branch turn's messages: it still
    // shows exactly turn 1's user+assistant pair. Before the fix the branch
    // turn's rows leaked onto main here.
    let (status, body) = get(&app, &format!("/api/threads/{main_thread}/messages")).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "main thread messages fetched: {body:?}"
    );
    let main_messages = body["messages"]
        .as_array()
        .expect("the main thread's messages response carries a messages array");
    assert!(
        main_messages
            .iter()
            .all(|m| m["thread_id"].as_i64() == Some(main_thread)),
        "every message on the main thread stays on main: {main_messages:?}"
    );
    assert!(
        main_messages
            .iter()
            .all(|m| m["content_text"] != json!("branch text")),
        "the branch prompt must NOT appear on the main thread: {main_messages:?}"
    );
    assert_eq!(
        main_messages
            .iter()
            .filter(|m| m["role"] == json!("user"))
            .count(),
        1,
        "main keeps only turn 1's single user prompt (the branch prompt did not \
         leak onto main): {main_messages:?}"
    );
    let main_user = main_messages
        .iter()
        .find(|m| m["role"] == json!("user"))
        .expect("turn 1's user prompt is on main");
    assert_eq!(
        main_user["content_text"],
        json!("first message"),
        "main's only user prompt is turn 1's opening message"
    );
}

/// A unique temp path for a shared on-disk SQLite database, removed on drop. The
/// restart test opens it from two separate backends (before/after the simulated
/// restart), so it must outlive both — unlike the in-memory store every other
/// test uses.
struct DbGuard {
    path: std::path::PathBuf,
}

impl DbGuard {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "fake-codex-restart-{}-{:?}.db",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::remove_file(&path).ok();
        Self { path }
    }

    fn open(&self) -> SqliteStore {
        SqliteStore::open(&self.path.to_string_lossy()).unwrap()
    }
}

impl Drop for DbGuard {
    fn drop(&mut self) {
        std::fs::remove_file(&self.path).ok();
        // WAL sidecar files.
        std::fs::remove_file(self.path.with_extension("db-wal")).ok();
        std::fs::remove_file(self.path.with_extension("db-shm")).ok();
    }
}

/// A one-turn scenario that streams `reply` from a distinct `turn_id`/`item_id`,
/// so two successive turns (the second after a restart) produce distinct,
/// non-colliding provider items. The provider thread id is fixed to `thr_restart`
/// across both, so the resume reattaches to the same thread.
///
/// `model` is what this backend's app-server reports for the thread. The two
/// halves of the restart test give distinct values, so the post-restart messages
/// prove the metadata came from the **resume** response rather than from
/// anything cached before the restart.
fn restart_turn_scenario(turn_id: &str, item_id: &str, reply: &str, model: &str) -> ScenarioGuard {
    ScenarioGuard::write(&format!(
        r#"{{
            "thread_id": "thr_restart",
            "model": "{model}",
            "turn": {{
                "turn_id": "{turn_id}",
                "emit": [
                    {{ "type": "turn_started" }},
                    {{ "type": "item_started",   "item": {{ "id": "{item_id}", "type": "agentMessage" }} }},
                    {{ "type": "item_completed", "item": {{ "id": "{item_id}", "type": "agentMessage", "text": "{reply}" }} }},
                    {{ "type": "turn_completed", "status": "completed" }}
                ]
            }}
        }}"#
    ))
}

/// The Codex **resume-across-restart** full loop: create a session and complete a
/// turn, then boot a SECOND backend over the SAME on-disk database with a fresh
/// interactor (no in-process bindings — the post-restart state) and a distinct
/// scenario, and send another message to the same thread.
///
/// This is the regression proof for dogfooding gap #1: after a server restart the
/// live `codex app-server` connection + thread + bound `open_agent` are gone, so
/// a send to a previously-created Codex session used to take the Claude resume
/// path (`ensure_open` → `claude --resume`) and fail with `ResumeUnavailable`
/// (surfaced as `409`). The fix reconnects the session over the adapter via
/// `thread/resume` (reattaching to the SAME provider thread) and re-seeds the
/// content source at the persisted message count, so the second turn dispatches,
/// streams, and completes, and the persisted conversation **continues**: the
/// first turn's assistant reply is preserved and the second's is appended with
/// the next sequence number — no renumber, no duplicate.
#[tokio::test(flavor = "multi_thread")]
async fn codex_resume_across_restart_continues_the_persisted_conversation() {
    const REPLY_ONE: &str = "reply from turn one";
    const REPLY_TWO: &str = "reply from turn two";
    // Distinct per backend, so a post-restart message reporting `MODEL_TWO` can
    // only have learned it from the `thread/resume` response.
    const MODEL_ONE: &str = "model-before-restart";
    const MODEL_TWO: &str = "model-after-restart";
    let db = DbGuard::new();

    // ---- Before the restart: create the session, complete turn 1. ----
    let scenario1 = restart_turn_scenario("turn_one", "item_one", REPLY_ONE, MODEL_ONE);
    let (thread_id, session_id) = {
        let (app, state) = build_app_with(db.open(), &scenario1);
        let mut events = state.subscribe();
        state
            .spawn_async_event_drain()
            .expect("the async drain is taken exactly once");

        let (status, body) = post_json(
            &app,
            "/api/sends",
            json!({ "new_session": true, "provider": "codex", "text": "first message" }),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "the first send was created: {body:?}"
        );
        let session_id = body["send"]["session_id"].as_str().unwrap().to_owned();
        let thread_id = body["send"]["thread_id"].as_i64().unwrap();

        // Let turn 1 stream and complete, so its assistant reply is persisted.
        drain_one_turn(&mut events, &session_id).await;
        let (status, body) = get(&app, &format!("/api/threads/{thread_id}/messages")).await;
        assert_eq!(status, StatusCode::OK, "turn 1 messages fetched: {body:?}");
        assert!(
            body["messages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|m| m["content_text"] == json!(REPLY_ONE)),
            "turn 1's assistant reply persisted before the restart: {body:?}"
        );
        (thread_id, session_id)
        // `app`/`state` (and the turn-1 `fake-codex` subprocess) drop here — the
        // server going away.
    };

    // ---- After the restart: a brand-new backend over the SAME database. ----
    let scenario2 = restart_turn_scenario("turn_two", "item_two", REPLY_TWO, MODEL_TWO);
    let (app, state) = build_app_with(db.open(), &scenario2);
    let mut events = state.subscribe();
    state
        .spawn_async_event_drain()
        .expect("the async drain is taken exactly once");

    // The second send targets the SAME thread. Before the fix this returned `409`
    // (ResumeUnavailable via the Claude path); after it must reconnect over the
    // adapter and be accepted.
    let (status, body) = post_json(
        &app,
        "/api/sends",
        json!({ "thread_id": thread_id, "text": "second message" }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "the post-restart send resumed the Codex session over the adapter (no 409): {body:?}"
    );
    assert_eq!(
        body["send"]["session_id"].as_str().unwrap(),
        session_id,
        "the resumed send stays on the same session"
    );

    // Turn 2 streams and completes over the reconnected pump.
    drain_one_turn(&mut events, &session_id).await;

    // The persisted conversation continued: turn 1's reply is still there and
    // turn 2's is appended, on contiguous sequence numbers with no duplicate —
    // proof the content source was re-seeded at the persisted count, not 0.
    let (status, body) = get(&app, &format!("/api/threads/{thread_id}/messages")).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "post-restart messages fetched: {body:?}"
    );
    let messages = body["messages"].as_array().unwrap();

    // All four messages of the two-turn conversation are present: turn 1's user
    // prompt + assistant reply (preserved across the restart) and turn 2's user
    // prompt + assistant reply (appended after the resume). None was overwritten.
    for expected in ["first message", REPLY_ONE, "second message", REPLY_TWO] {
        assert!(
            messages
                .iter()
                .any(|m| m["content_text"] == json!(expected)),
            "`{expected}` is present in the continued conversation: {messages:?}"
        );
    }
    assert_eq!(
        messages.len(),
        4,
        "the conversation has exactly its four messages — nothing lost or duplicated: {messages:?}"
    );

    // Sequence numbers are contiguous with no duplicate: history was extended,
    // not renumbered.
    let mut seqs: Vec<i64> = messages
        .iter()
        .map(|m| m["seq"].as_i64().unwrap())
        .collect();
    seqs.sort_unstable();
    assert_eq!(
        seqs,
        vec![0, 1, 2, 3],
        "sequence numbers continue contiguously across the resume: {messages:?}"
    );

    // A resumed session still reports its provider metadata: `thread/resume`
    // carries the same required top-level `model` as `thread/start`, so the
    // post-restart turn's messages are stamped from the resume response — the
    // second backend's model, never left blank and never the pre-restart one.
    // The pre-restart messages keep the model that was running when they were
    // folded, since each row records what produced it.
    let model_of = |text: &str| {
        messages
            .iter()
            .find(|m| m["content_text"] == json!(text))
            .unwrap_or_else(|| panic!("`{text}` is present"))["model"]
            .clone()
    };
    assert_eq!(
        model_of(REPLY_TWO),
        json!(MODEL_TWO),
        "the resumed turn reports the model its `thread/resume` announced: {messages:?}"
    );
    assert_eq!(
        model_of(REPLY_ONE),
        json!(MODEL_ONE),
        "the pre-restart turn keeps the model that produced it: {messages:?}"
    );
}

/// The Codex interrupt full loop: browser → server → `fake-codex`.
///
/// The scenario's turn emits only `turn_started`, so it never self-completes —
/// the turn stays in flight until something interrupts it. The test drives one
/// turn to that in-flight state, issues `POST /api/sessions/{id}/interrupt`, and
/// asserts the interrupt settles the turn over the broadcast: the fake handles
/// `turn/interrupt` (answering `{}` then emitting `turn/completed{interrupted}`),
/// the translate layer maps that to an interrupted turn end, and the event pump
/// drives the session actor to emit `TurnInterrupted` — reaching the same
/// broadcast the WebSocket forwards. The session is NOT closed by the interrupt:
/// its event pump must stay alive to receive the interrupted completion, so a
/// follow-up `GET /api/sessions` shows the session still open.
///
/// `fake-codex` needs no changes for this — it already handles `turn/interrupt`.
#[tokio::test(flavor = "multi_thread")]
async fn codex_interrupt_settles_the_in_flight_turn_over_the_full_stack() {
    // A turn that emits only `turn_started`: it stays in flight, so the only
    // completion is the interrupted one the interrupt produces.
    let scenario = ScenarioGuard::write(
        r#"{
            "thread_id": "thr_interrupt_loop",
            "turn": {
                "turn_id": "turn_interrupt_loop",
                "emit": [
                    { "type": "turn_started" }
                ]
            }
        }"#,
    );

    let (app, state) = build_app(&scenario);
    // Subscribe and start the async-seam drain BEFORE the prompt, so no event the
    // pump emits after the send returns can be missed.
    let mut events = state.subscribe();
    state
        .spawn_async_event_drain()
        .expect("the async drain is taken exactly once");

    // Create a Codex session with a first prompt over the REST surface. The turn
    // starts and stays in flight (the scenario emits nothing that completes it).
    let (status, body) = post_json(
        &app,
        "/api/sends",
        json!({ "new_session": true, "provider": "codex", "text": "start a long task" }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "the send was created: {body:?}"
    );
    let session_id = body["send"]["session_id"]
        .as_str()
        .expect("the send response carries its session id")
        .to_owned();

    // Interrupt the in-flight turn over the REST surface.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/sessions/{session_id}/interrupt"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::NO_CONTENT,
        "the interrupt was accepted"
    );

    // The interrupt settles the turn over the broadcast: the fake's
    // `turn/completed{interrupted}` drives the pump to a `TurnInterrupted`.
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    loop {
        let event = tokio::time::timeout_at(deadline, events.recv())
            .await
            .expect("timed out waiting for the interrupt to settle the turn")
            .expect("the broadcast channel stayed open");
        if let SessionEvent::TurnInterrupted {
            session_id: sid, ..
        } = event
        {
            assert_eq!(
                sid.as_str(),
                session_id,
                "the interrupt settlement names our session"
            );
            break;
        }
    }

    // The session was NOT closed by the interrupt: it is still open, so its event
    // pump survived to receive the interrupted completion in the first place.
    let (status, body) = get(&app, "/api/sessions").await;
    assert_eq!(status, StatusCode::OK, "sessions listed: {body:?}");
    let session = body["sessions"]
        .as_array()
        .expect("the sessions response carries a sessions array")
        .iter()
        .find(|s| s["session"]["id"] == json!(session_id))
        .expect("our session is listed");
    assert_eq!(
        session["open"],
        json!(true),
        "the session stays open after an interrupt (the pump was not torn down)"
    );
}

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
/// the fake received the exact `accept`/`decline` — it echoes the received
/// decision as an assistant message, which the test reads back from the
/// persisted transcript.
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

    // The fake received the exact accept/decline: it echoes the decision it was
    // handed as an assistant message, which persisted through the same content
    // path as any other reply.
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

/// The comms-log stream, over the same real stack: a browser joining a live
/// session receives the frames that already flew and then the next live one.
///
/// This is the endpoint's contract asserted end to end — the frames come from a
/// real adapter driving a real `fake-codex` over a real turn, and they are read
/// through the exact subscription the `/comms` route pumps into its socket. Only
/// the WebSocket bytes are left out (the handler does nothing but serialize each
/// frame and write it), which keeps the test free of a WebSocket client
/// dependency without weakening what it proves.
#[tokio::test(flavor = "multi_thread")]
async fn the_comms_log_replays_a_live_sessions_frames_then_tails_new_ones() {
    let scenario = ScenarioGuard::write(&format!(
        r#"{{
            "thread_id": "thr_comms_loop",
            "turn": {{
                "turn_id": "turn_comms_loop",
                "emit": [
                    {{ "type": "turn_started" }},
                    {{ "type": "item_completed", "item": {{ "id": "item_1", "type": "agentMessage", "text": "{REPLY}" }} }},
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

    // A first turn runs to completion, so by the time we look there is history.
    let (status, body) = post_json(
        &app,
        "/api/sends",
        json!({ "new_session": true, "provider": "codex", "text": "hello codex" }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "the send was created: {body:?}"
    );
    let session_id = body["send"]["session_id"]
        .as_str()
        .expect("the send response carries its session id")
        .to_owned();
    let thread_id = body["send"]["thread_id"]
        .as_i64()
        .expect("the send response carries its main thread id");
    await_turn_completion(&mut events).await;

    // Now the browser opens the pane — mid-session, after the frames flew.
    let mut watcher = state.watch_comms_log(&session_id);

    // The replay: the session's own launch first, then the turn's pushed flow,
    // strictly ordered. Draining to the end of the buffer (rather than reading a
    // fixed count) is what makes the assertion independent of how many frames the
    // scenario happens to emit.
    let replayed = drain_buffered_comms(&mut watcher).await;
    let methods: Vec<Option<&str>> = replayed
        .iter()
        .map(|frame| frame.method.as_deref())
        .collect();
    assert_eq!(
        methods.first().copied().flatten(),
        Some("thread/start"),
        "the replay starts at the session's launch: {methods:?}"
    );
    assert!(
        methods.contains(&Some("turn/completed")),
        "the replay includes the completed turn's pushed frame: {methods:?}"
    );
    let seqs: Vec<u64> = replayed.iter().map(|frame| frame.seq).collect();
    assert!(
        seqs.windows(2).all(|pair| pair[0] < pair[1]),
        "replayed frames are strictly ordered: {seqs:?}"
    );
    let last_replayed_seq = *seqs.last().expect("the replay is non-empty");

    // And then the live tail: a second prompt on the same session must show up on
    // the SAME subscription, numbered after the replay — the handoff a client
    // connecting mid-session depends on.
    let (status, body) = post_json(
        &app,
        "/api/sends",
        json!({ "thread_id": thread_id, "text": "second message" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "the second send: {body:?}");

    let live = tokio::time::timeout(TIMEOUT, async {
        loop {
            let frame = watcher.next().await.expect("the stream stayed open");
            if frame.method.as_deref() == Some("turn/start") {
                return frame;
            }
        }
    })
    .await
    .expect("timed out waiting for the second turn's frame on the live tail");
    assert!(
        live.seq > last_replayed_seq,
        "the live frame continues the replay's numbering ({} > {last_replayed_seq})",
        live.seq
    );
    assert_eq!(live.direction, delta_wire::WireCommsDirection::ToAgent);
    assert_eq!(live.kind, delta_wire::WireCommsFrameKind::Request);
}

/// A session with no adapter behind it (never launched, so nothing was ever
/// recorded) gets an open, quiet stream rather than an error — the pane shows its
/// idle state, which is the honest answer for "nothing is being exchanged".
#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_session_gets_an_idle_stream_rather_than_a_failure() {
    let scenario = ScenarioGuard::write(r#"{ "thread_id": "thr_idle" }"#);
    let (_app, state) = build_app(&scenario);

    let mut watcher = state.watch_comms_log("sess-never-launched");
    assert!(
        tokio::time::timeout(Duration::from_millis(50), watcher.next())
            .await
            .is_err(),
        "the stream is open and simply has nothing to say"
    );
}

/// Pump the broadcast until the turn completes, so a test can line up against a
/// finished turn without re-implementing the wait.
async fn await_turn_completion(events: &mut tokio::sync::broadcast::Receiver<SessionEvent>) {
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

/// Read the frames a fresh subscription already had buffered, stopping when it
/// goes quiet (there is no in-band "end of replay" marker — by design, since the
/// stream is one continuous sequence).
async fn drain_buffered_comms(
    watcher: &mut delta_server::CommsSubscription,
) -> Vec<delta_wire::WireCommsFrame> {
    let mut frames = Vec::new();
    while let Ok(Some(frame)) =
        tokio::time::timeout(Duration::from_millis(200), watcher.next()).await
    {
        frames.push(frame);
    }
    frames
}

/// The usage loop: a Codex turn's token accounting and its account's rate
/// limits reach the browser broadcast as `StatusUpdated` snapshots, over the
/// same real stack.
///
/// The rate-limit half is the load-bearing one. `account/rateLimits/updated`
/// carries **no `threadId`** — this scenario emits it through the fake's
/// `account_notification` step precisely so it does not — so the transport
/// cannot demux it to a session and it takes the connection-level unrouted
/// path. Reaching this assertion therefore proves the whole chain: the drain
/// the adapter owns, the fan-out onto a live session's stream, the pump, and
/// the broadcast.
///
/// Both snapshots are observability only, which is asserted too: nothing here
/// is persisted, so the conversation is exactly the turn's own message.
#[tokio::test(flavor = "multi_thread")]
async fn codex_usage_and_account_rate_limits_reach_the_browser_broadcast() {
    let scenario = ScenarioGuard::write(
        r#"{
            "thread_id": "thr_usage",
            "turn": {
                "turn_id": "turn_usage",
                "emit": [
                    { "type": "turn_started" },
                    { "type": "item_completed", "item": { "id": "item_1", "type": "agentMessage", "text": "counted" } },
                    { "type": "notification", "method": "thread/tokenUsage/updated",
                      "params": { "turnId": "turn_usage", "tokenUsage": {
                          "total": { "totalTokens": 500000, "inputTokens": 480000, "cachedInputTokens": 400000,
                                     "outputTokens": 20000, "reasoningOutputTokens": 5000 },
                          "last": { "totalTokens": 50000, "inputTokens": 48000, "cachedInputTokens": 40000,
                                    "outputTokens": 2000, "reasoningOutputTokens": 500 },
                          "modelContextWindow": 200000 } } },
                    { "type": "account_notification", "method": "account/rateLimits/updated",
                      "params": { "rateLimits": {
                          "primary": { "usedPercent": 21, "resetsAt": 1700000000, "windowDurationMins": 300 },
                          "secondary": { "usedPercent": 4, "resetsAt": 1700500000, "windowDurationMins": 10080 },
                          "planType": "pro" } } },
                    { "type": "turn_completed", "status": "completed" }
                ]
            }
        }"#,
    );

    let (app, state) = build_app(&scenario);
    let mut events = state.subscribe();
    state
        .spawn_async_event_drain()
        .expect("the async drain is taken exactly once");

    let (status, body) = post_json(
        &app,
        "/api/sends",
        json!({ "new_session": true, "provider": "codex", "text": "count my tokens" }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "the send was created: {body:?}"
    );
    let session_id = body["send"]["session_id"]
        .as_str()
        .expect("the send response carries its session id")
        .to_owned();

    // Collect status snapshots until both halves have been seen. They arrive on
    // independent paths (the thread demux and the connection drain), so the
    // order between them is not guaranteed and must not be asserted.
    let mut context_snapshot = None;
    let mut rate_limit_snapshot = None;
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    while context_snapshot.is_none() || rate_limit_snapshot.is_none() {
        let event = tokio::time::timeout_at(deadline, events.recv())
            .await
            .expect("timed out waiting for the Codex usage snapshots")
            .expect("the broadcast channel stayed open");
        if let SessionEvent::StatusUpdated {
            session_id: sid,
            snapshot,
        } = event
        {
            assert_eq!(sid.as_str(), session_id, "the snapshot names our session");
            assert_eq!(
                snapshot.provider,
                delta_usecase::AgentProvider::Codex,
                "a Codex snapshot says so, so the browser cannot file it under Claude"
            );
            if snapshot.context_used_percentage.is_some() {
                context_snapshot = Some(snapshot);
            } else if snapshot.rate_limits.is_some() {
                rate_limit_snapshot = Some(snapshot);
            }
        }
    }

    let context = context_snapshot.expect("a context-usage snapshot");
    assert_eq!(
        context.context_used_percentage,
        Some(25.0),
        "the last call's 50k of a 200k window, computed at the Codex edge"
    );
    assert_eq!(context.context_current_usage, Some(50_000));
    assert_eq!(
        context.rate_limits, None,
        "a token-usage frame states nothing about rate limits, so it cannot clear them"
    );

    let windows = rate_limit_snapshot
        .expect("a rate-limit snapshot")
        .rate_limits
        .expect("the account's windows");
    assert_eq!(
        windows.len(),
        2,
        "both account windows crossed the unrouted path: {windows:?}"
    );
    assert_eq!(windows[0].duration_seconds, Some(5 * 60 * 60));
    assert_eq!(windows[0].used_percentage, Some(21.0));
    assert_eq!(windows[1].duration_seconds, Some(7 * 24 * 60 * 60));
    assert_eq!(windows[1].used_percentage, Some(4.0));

    // Observability only: the usage frames persisted nothing, so the thread
    // holds exactly the turn's own prompt and reply.
    let thread_id = body["send"]["thread_id"].as_i64().unwrap();
    let (status, messages) = get(&app, &format!("/api/threads/{thread_id}/messages")).await;
    assert_eq!(status, StatusCode::OK);
    let texts: Vec<&str> = messages["messages"]
        .as_array()
        .expect("a message list")
        .iter()
        .filter_map(|message| message["content"][0]["text"].as_str())
        .collect();
    assert_eq!(
        texts,
        vec!["count my tokens", "counted"],
        "usage frames add no messages of their own"
    );
}
