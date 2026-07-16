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
//! turn-complete loop, offline and deterministic. Permission and interrupt are
//! deliberately out of scope for this slice.

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
        Self { dir, path }
    }
}

impl Drop for ScenarioGuard {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).ok();
    }
}

/// Assemble the real backend wired to drive the `fake-codex` binary with
/// `scenario`, returning the router and the shared state (whose broadcast the
/// test subscribes to).
fn build_app(scenario: &ScenarioGuard) -> (Router, AppState) {
    let store = SqliteStore::open_in_memory().unwrap();
    let codex_config = CodexLaunchConfig {
        // The fake IS the app-server, so it takes no `app-server` argument.
        codex_bin: env!("CARGO_BIN_EXE_fake-codex").to_owned(),
        args: vec![],
        // Hand the fake its scenario through the child's env, not the parent's.
        env: vec![(
            "FAKE_CODEX_SCENARIO".to_owned(),
            scenario.path.to_string_lossy().into_owned(),
        )],
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
    // A scripted turn: a streaming fragment, the completed assistant message,
    // then a clean turn completion. The started item carries a strict prefix of
    // the completed text, so it translates to a live `AssistantDelta` while the
    // completed item translates to the persisted `AssistantMessage`.
    let scenario = ScenarioGuard::write(&format!(
        r#"{{
            "thread_id": "thr_full_loop",
            "turn": {{
                "turn_id": "turn_full_loop",
                "emit": [
                    {{ "type": "turn_started" }},
                    {{ "type": "item_started",   "item": {{ "id": "item_1", "itemType": "agent_message", "text": "{REPLY_FRAGMENT}" }} }},
                    {{ "type": "item_completed", "item": {{ "id": "item_1", "itemType": "agent_message", "text": "{REPLY}" }} }},
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
    // The user prompt persisted too, so the loop is a real conversation.
    assert!(
        messages.iter().any(|m| m["role"] == json!("user")),
        "the user prompt was persisted as well"
    );
}
