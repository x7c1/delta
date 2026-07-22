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
use delta_server::{router, AppState};
use delta_sqlite::SqliteStore;
use delta_transcript::JsonlTranscript;
use delta_usecase::{
    AgentAdapterFactory, GitWorktree, Interactor, SessionEvent, TmuxDriver, Transcript, Workspace,
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
        Self {
            dir,
            path,
            inject_log,
        }
    }

    /// The `thread/inject_items` payloads the fake recorded, one per line,
    /// parsed as JSON. Empty when the fake was never asked to inject (the file
    /// is created lazily on the first injection).
    fn injected_items(&self) -> Vec<Value> {
        match std::fs::read_to_string(&self.inject_log) {
            Ok(contents) => contents
                .lines()
                .filter(|line| !line.trim().is_empty())
                .map(|line| serde_json::from_str(line).expect("recorded inject line is JSON"))
                .collect(),
            Err(_) => Vec::new(),
        }
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
        // Hand the fake its scenario and inject-record path through the child's
        // env, not the parent's.
        env: vec![
            (
                "FAKE_CODEX_SCENARIO".to_owned(),
                scenario.path.to_string_lossy().into_owned(),
            ),
            (
                "FAKE_CODEX_INJECT_LOG".to_owned(),
                scenario.inject_log.to_string_lossy().into_owned(),
            ),
        ],
    };
    let factory: Arc<dyn AgentAdapterFactory> = Arc::new(CodexAdapterFactory::new(codex_config));

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
    .with_codex_adapter_factory(factory);

    let state = AppState::from_interactor(interactor, "delta-codex-full-loop");
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
fn restart_turn_scenario(turn_id: &str, item_id: &str, reply: &str) -> ScenarioGuard {
    ScenarioGuard::write(&format!(
        r#"{{
            "thread_id": "thr_restart",
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
    let db = DbGuard::new();

    // ---- Before the restart: create the session, complete turn 1. ----
    let scenario1 = restart_turn_scenario("turn_one", "item_one", REPLY_ONE);
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
    let scenario2 = restart_turn_scenario("turn_two", "item_two", REPLY_TWO);
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
