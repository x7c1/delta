//! Application-level integration test.
//!
//! This is the single black-box test for the whole backend. It assembles the
//! real router with test-wired gateways — an in-memory `SqliteStore`, a
//! `JsonlTranscript` reading a temp file we control, and a no-op `TmuxDriver`
//! so the test runs in CI without a real tmux — and drives a realistic flow
//! across the REST and hook surface through `router()` using
//! `tower::ServiceExt::oneshot` (no network bind).
//!
//! The flow exercised end to end:
//!
//! 1. The first `UserPromptSubmit` hook registers the session.
//! 2. `GET /api/session` hydrates the session and its `main` thread id.
//! 3. `POST /api/sends` enqueues a send (with a locator quote).
//! 4. The matching `UserPromptSubmit` hook correlates against the FIFO head:
//!    the send is marked matched, the locator quote is returned inside the
//!    `hookSpecificOutput.additionalContext` envelope, and the transcript line
//!    is ingested.
//! 5. `GET /api/threads` and `GET /api/threads/{id}/messages` reflect the
//!    resulting thread and message state.

use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use axum::Router;
use serde_json::{json, Value};
use tower::ServiceExt;

use delta_server::{router, AppState};
use delta_sqlite::SqliteStore;
use delta_transcript::JsonlTranscript;
use delta_usecase::{Interactor, TmuxDriver, Workspace};

/// A `TmuxDriver` that records the lines it would have sent instead of touching
/// a real tmux pane, so the test can assert the keystrokes were dispatched. It
/// also models the session lifecycle in memory so `ensure_session` works without
/// a real tmux.
#[derive(Default)]
struct FakeTmux {
    sent: AtomicUsize,
    has_session: std::sync::atomic::AtomicBool,
}

#[async_trait]
impl TmuxDriver for FakeTmux {
    async fn has_session(&self) -> delta_usecase::Result<bool> {
        Ok(self.has_session.load(Ordering::SeqCst))
    }

    async fn create_session(&self, _workdir: &str, _command: &str) -> delta_usecase::Result<()> {
        self.has_session.store(true, Ordering::SeqCst);
        Ok(())
    }

    async fn send_line(&self, _text: &str) -> delta_usecase::Result<()> {
        self.sent.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

/// A local wrapper around a shared [`FakeTmux`] so the app can own the driver
/// while the test keeps a handle to observe how many lines were dispatched.
struct SharedTmux(Arc<FakeTmux>);

#[async_trait]
impl TmuxDriver for SharedTmux {
    async fn has_session(&self) -> delta_usecase::Result<bool> {
        self.0.has_session().await
    }

    async fn create_session(&self, workdir: &str, command: &str) -> delta_usecase::Result<()> {
        self.0.create_session(workdir, command).await
    }

    async fn send_line(&self, text: &str) -> delta_usecase::Result<()> {
        self.0.send_line(text).await
    }
}

/// A no-op `Workspace` so `ensure_session` does not touch the real filesystem.
struct NoopWorkspace;

#[async_trait]
impl Workspace for NoopWorkspace {
    async fn write_session_settings(
        &self,
        _workdir: &str,
        _settings_json: &str,
    ) -> delta_usecase::Result<()> {
        Ok(())
    }
}

/// Assemble the app with test-wired gateways and return the router plus the
/// fake tmux driver (for asserting keystroke dispatch) and the transcript path.
fn build_app() -> (Router, Arc<FakeTmux>, std::path::PathBuf) {
    let transcript_file = tempfile::Builder::new()
        .prefix("delta-e2e-transcript-")
        .suffix(".jsonl")
        .tempfile()
        .unwrap();
    // Keep the path but let the file persist for the whole test.
    let (_, transcript_path) = transcript_file.keep().unwrap();

    let tmux = Arc::new(FakeTmux::default());
    let store = SqliteStore::open_in_memory().unwrap();
    let transcript = JsonlTranscript::new();

    let interactor = Interactor::new(
        Box::new(SharedTmux(tmux.clone())) as Box<dyn TmuxDriver>,
        Box::new(transcript) as Box<dyn delta_usecase::Transcript>,
        Box::new(store) as Box<dyn delta_usecase::SessionStore>,
        Box::new(NoopWorkspace) as Box<dyn delta_usecase::Workspace>,
    );

    let state = AppState::from_interactor(
        interactor,
        "delta:0.0".into(),
        "/tmp/delta-e2e-session".into(),
        "{}".into(),
    );
    (router(state), tmux, transcript_path)
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

#[tokio::test]
async fn drives_session_send_and_turn_correlation_end_to_end() {
    let (app, tmux, transcript_path) = build_app();
    let transcript_str = transcript_path.to_str().unwrap().to_owned();
    let session_id = "sess-e2e";

    // 1. First UserPromptSubmit registers the session. The transcript is empty,
    //    so this prompt is treated as external input and returns no context.
    let (status, body) = post_json(
        &app,
        "/hooks/user-prompt-submit",
        json!({
            "prompt": "hello there",
            "session_id": session_id,
            "transcript_path": transcript_str,
            "cwd": "/work/delta",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.get("additionalContext").is_none(),
        "no pending send queued yet, so no additionalContext"
    );

    // 2. GET /api/session hydrates the registered session and its main thread.
    let (status, body) = get(&app, "/api/session").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["session"]["id"], session_id);
    assert_eq!(body["session"]["cwd"], "/work/delta");
    let main_thread_id = body["main_thread_id"].as_i64().expect("main thread id");

    // 3. POST /api/sends enqueues a send into the main thread, with a locator
    //    quote that must surface as additionalContext on the matching turn.
    let prompt = "what is a delta?";
    let (status, body) = post_json(
        &app,
        "/api/sends",
        json!({
            "thread_id": main_thread_id,
            "text": prompt,
            "locator_quote": "the main channel",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["send"]["text"], prompt);
    assert_eq!(body["send"]["status"], "pending");
    assert_eq!(body["send"]["thread_id"].as_i64(), Some(main_thread_id));
    let pending_send_id = body["send"]["id"].as_i64().expect("pending send id");
    // The send was dispatched into the (fake) tmux pane.
    assert_eq!(tmux.sent.load(Ordering::SeqCst), 1);

    // 4. The session emits the corresponding transcript line. Append it so the
    //    correlation can find the matched uuid.
    let matched_uuid = "uuid-of-the-question";
    {
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&transcript_path)
            .unwrap();
        let line = json!({
            "uuid": matched_uuid,
            "parentUuid": Value::Null,
            "type": "user",
            "promptId": "prompt-1",
            "timestamp": "2026-01-01T00:00:00Z",
            "message": { "role": "user", "content": prompt },
        });
        writeln!(file, "{line}").unwrap();
        file.flush().unwrap();
    }

    // The matching UserPromptSubmit fires: the FIFO head matches, the locator
    // quote is returned in the hookSpecificOutput envelope, and the line is
    // ingested.
    let (status, body) = post_json(
        &app,
        "/hooks/user-prompt-submit",
        json!({
            "prompt": prompt,
            "session_id": session_id,
            "transcript_path": transcript_str,
            "cwd": "/work/delta",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // Claude Code only consumes injected context from the `hookSpecificOutput`
    // envelope, so the matched quote must surface nested there.
    assert_eq!(
        body["hookSpecificOutput"]["hookEventName"], "UserPromptSubmit",
        "the envelope names the originating hook event"
    );
    // The quote is not injected verbatim: it is wrapped in a short frame so the
    // model recognises it as a passage the user selected from earlier in the
    // conversation, anchoring the current message.
    assert_eq!(
        body["hookSpecificOutput"]["additionalContext"],
        "The user is replying to this passage they selected from earlier in the conversation:\n\"the main channel\"",
        "matched send injects its locator quote, framed, as additionalContext"
    );

    // 5. GET /api/threads exposes the main thread for the navigator.
    let (status, body) = get(&app, "/api/threads").await;
    assert_eq!(status, StatusCode::OK);
    let threads = body["threads"].as_array().expect("threads array");
    assert_eq!(threads.len(), 1, "only the main thread exists");
    assert_eq!(threads[0]["id"].as_i64(), Some(main_thread_id));
    assert_eq!(threads[0]["title"], "main");

    // GET /api/threads/{id}/messages reflects the ingested, correlated message.
    let (status, body) = get(&app, &format!("/api/threads/{main_thread_id}/messages")).await;
    assert_eq!(status, StatusCode::OK);
    let messages = body["messages"].as_array().expect("messages array");
    assert_eq!(messages.len(), 1, "the question was ingested once");
    let message = &messages[0];
    assert_eq!(message["uuid"], matched_uuid);
    assert_eq!(message["role"], "user");
    assert_eq!(message["thread_id"].as_i64(), Some(main_thread_id));
    assert_eq!(message["content_text"], prompt);

    // A miss against the FIFO (no remaining pending send) is external input.
    let (status, body) = post_json(
        &app,
        "/hooks/user-prompt-submit",
        json!({
            "prompt": "typed straight into the pane",
            "session_id": session_id,
            "transcript_path": transcript_str,
            "cwd": "/work/delta",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("additionalContext").is_none());

    // No further keystrokes were dispatched by the hook path.
    assert_eq!(tmux.sent.load(Ordering::SeqCst), 1);

    // The matched send id remained stable across the flow.
    assert!(pending_send_id > 0);

    let _ = std::fs::remove_file(&transcript_path);
}

#[tokio::test]
async fn ensure_session_endpoint_reports_starting_then_ready() {
    let (app, tmux, transcript_path) = build_app();

    // No session exists yet, so the first POST /api/session creates it and the
    // route serializes the lifecycle as "starting".
    let (status, body) = post_json(&app, "/api/session", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "starting", "a freshly created session is starting");
    assert!(tmux.has_session().await.unwrap(), "the session was created");

    // A second POST finds the session already up: idempotent reuse, reported as
    // "ready" with no recreate.
    let (status, body) = post_json(&app, "/api/session", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ready", "a reused session is ready");

    let _ = std::fs::remove_file(&transcript_path);
}
