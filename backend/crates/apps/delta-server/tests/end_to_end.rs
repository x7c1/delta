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
//! 2. `GET /api/sessions` lists it, annotated with its open state and `main`
//!    thread id.
//! 3. `POST /api/sends` enqueues a send into that thread (with a locator quote),
//!    which routes to the thread's owning session.
//! 4. The matching `UserPromptSubmit` hook correlates against the FIFO head:
//!    the send is marked matched, the locator quote is returned inside the
//!    `hookSpecificOutput.additionalContext` envelope, and the transcript line
//!    is ingested.
//! 5. `GET /api/sessions/{id}/threads` and `GET /api/threads/{id}/messages`
//!    reflect the resulting thread and message state.
//! 6. `POST /api/sessions/{id}/close` tears the pane down; the session then
//!    lists as closed, and open/close/threads on an unknown id are `404`.

use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

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
    /// The number of lines dispatched via `send_line`.
    sent: AtomicUsize,
    /// The number of sessions spawned via `create_session`.
    created: AtomicUsize,
    /// The `command` argv of the most recent `create_session` call, so a test
    /// can assert the first prompt rides on the launch command line.
    last_command: std::sync::Mutex<Vec<String>>,
    /// The keystroke sequences injected via `send_keys`, in order, so a test
    /// can assert the exact keys an answered question drove into the pane.
    keyed: std::sync::Mutex<Vec<Vec<String>>>,
}

#[async_trait]
impl TmuxDriver for FakeTmux {
    async fn has_session(&self, _name: &str) -> delta_usecase::Result<bool> {
        Ok(false)
    }

    async fn create_session(
        &self,
        _name: &str,
        _workdir: &str,
        command: &[String],
    ) -> delta_usecase::Result<()> {
        self.created.fetch_add(1, Ordering::SeqCst);
        *self.last_command.lock().unwrap() = command.to_vec();
        Ok(())
    }

    async fn send_line(&self, _pane: &str, _text: &str) -> delta_usecase::Result<()> {
        self.sent.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn send_keys(&self, _pane: &str, keys: &[&str]) -> delta_usecase::Result<()> {
        self.keyed
            .lock()
            .unwrap()
            .push(keys.iter().map(|k| (*k).to_owned()).collect());
        Ok(())
    }

    async fn clear_input(&self, _pane: &str) -> delta_usecase::Result<()> {
        Ok(())
    }

    async fn kill_session(&self, _name: &str) -> delta_usecase::Result<()> {
        Ok(())
    }
}

/// A local wrapper around a shared [`FakeTmux`] so the app can own the driver
/// while the test keeps a handle to observe how many lines were dispatched.
struct SharedTmux(Arc<FakeTmux>);

#[async_trait]
impl TmuxDriver for SharedTmux {
    async fn has_session(&self, name: &str) -> delta_usecase::Result<bool> {
        self.0.has_session(name).await
    }

    async fn create_session(
        &self,
        name: &str,
        workdir: &str,
        command: &[String],
    ) -> delta_usecase::Result<()> {
        self.0.create_session(name, workdir, command).await
    }

    async fn send_line(&self, pane: &str, text: &str) -> delta_usecase::Result<()> {
        self.0.send_line(pane, text).await
    }

    async fn send_keys(&self, pane: &str, keys: &[&str]) -> delta_usecase::Result<()> {
        self.0.send_keys(pane, keys).await
    }

    async fn clear_input(&self, pane: &str) -> delta_usecase::Result<()> {
        self.0.clear_input(pane).await
    }

    async fn kill_session(&self, name: &str) -> delta_usecase::Result<()> {
        self.0.kill_session(name).await
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

    async fn resolve_existing_dir(&self, path: &str) -> delta_usecase::Result<String> {
        // The end-to-end tests never request a user-selected workdir, so a real
        // resolution is unnecessary; echo the path back as already-canonical.
        Ok(path.to_owned())
    }

    async fn list_dirs(&self, path: &str) -> delta_usecase::Result<delta_usecase::DirListing> {
        Ok(delta_usecase::DirListing {
            path: path.to_owned(),
            parent: None,
            entries: Vec::new(),
        })
    }
}

/// Assemble the app with test-wired gateways and return the router plus the
/// fake tmux driver (for asserting keystroke dispatch) and the transcript path.
fn build_app() -> (Router, Arc<FakeTmux>, std::path::PathBuf, AppState) {
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
        "/tmp/delta-e2e-session",
        "{}",
        "/tmp/delta-e2e-settings.json",
    );

    let state = AppState::from_interactor(interactor, "delta-e2e");
    (router(state.clone()), tmux, transcript_path, state)
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
    let (app, tmux, transcript_path, state) = build_app();
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

    // That first prompt started a turn; complete it with a Stop so the session
    // is idle before the next send. Otherwise the quoted send below would be
    // queued (held back behind the in-flight turn) rather than dispatched.
    let (status, _) = post_json(
        &app,
        "/hooks/stop",
        json!({ "session_id": session_id }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // 2. GET /api/sessions lists the registered session, annotated with its open
    //    state and main thread id. It registered via a hook (not a Delta spawn),
    //    so it is a known-but-closed data session.
    let (status, body) = get(&app, "/api/sessions").await;
    assert_eq!(status, StatusCode::OK);
    let sessions = body["sessions"].as_array().expect("sessions array");
    assert_eq!(sessions.len(), 1, "exactly the one registered session");
    assert_eq!(sessions[0]["session"]["id"], session_id);
    assert_eq!(sessions[0]["session"]["cwd"], "/work/delta");
    assert_eq!(
        sessions[0]["open"], false,
        "a hook-registered (external) session has no live pane"
    );
    assert!(
        sessions[0]["last_activity_at"].is_null(),
        "a freshly registered session has no messages yet"
    );
    assert!(
        body["next_cursor"].is_null(),
        "a single short page has no next cursor"
    );
    let main_thread_id = sessions[0]["main_thread_id"]
        .as_i64()
        .expect("main thread id");

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
    assert_eq!(body["send"]["status"], "dispatched");
    assert_eq!(body["send"]["thread_id"].as_i64(), Some(main_thread_id));
    let send_id = body["send"]["id"].as_i64().expect("dispatched send id");
    // The session was hook-registered (closed), so this send resumed it via
    // `claude --resume`. A resumed session holds its first prompt until its
    // `SessionStart(source=resume)` readiness hook arrives, so the keystroke has
    // NOT been dispatched yet.
    assert_eq!(
        tmux.sent.load(Ordering::SeqCst),
        0,
        "the resume's first prompt is held until SessionStart(resume)"
    );

    // GET /api/sessions/{id}/sends mirrors the queue: the dispatched send is
    // the session's one open send, so the browser's pending strip can render
    // it from server state.
    let (status, body) = get(&app, &format!("/api/sessions/{session_id}/sends")).await;
    assert_eq!(status, StatusCode::OK);
    let sends = body["sends"].as_array().expect("sends array");
    assert_eq!(sends.len(), 1, "the dispatched send is open");
    assert_eq!(sends[0]["id"].as_i64(), Some(send_id));
    assert_eq!(sends[0]["text"], prompt);
    assert_eq!(sends[0]["status"], "dispatched");
    assert_eq!(sends[0]["thread_id"].as_i64(), Some(main_thread_id));

    // Feed the readiness hook. SessionStart(source=resume) blocks `claude` until
    // its handler returns, so the handler only *marks* the resume ready — it does
    // NOT type the held prompt (a keystroke sent from inside the hook would be
    // lost to a still-blocked TUI). So nothing is dispatched yet.
    let (status, _) = post_json(
        &app,
        "/hooks/session-start",
        json!({
            "session_id": session_id,
            "source": "resume",
            "transcript_path": transcript_str,
            "cwd": "/work/delta",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        tmux.sent.load(Ordering::SeqCst),
        0,
        "the readiness hook only marks ready; it does not dispatch from the handler"
    );

    // The held first prompt is dispatched on the background tick, a beat after the
    // hook returned and `claude` is input-ready. Drive that tick directly with a
    // `now` past the dispatch settle (the server loop normally calls this with
    // `Instant::now()`); the keystroke now lands on the normal `send_line` path.
    state
        .interactor()
        .dispatch_ready_resumes(Instant::now() + Duration::from_secs(1))
        .await
        .unwrap();
    assert_eq!(
        tmux.sent.load(Ordering::SeqCst),
        1,
        "the held first prompt dispatched on the settle tick once the resume was ready"
    );

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
    // conversation, anchoring the current message. Because the send carries a
    // locator quote, the frame is followed by a note binding that passage to the
    // thread the conversation is now in (`thread:N`).
    assert_eq!(
        body["hookSpecificOutput"]["additionalContext"],
        format!(
            "The user is replying to this passage they selected from earlier in the conversation:\n\"the main channel\"\nThat passage starts a separate thread (thread:{main_thread_id}); the user is now talking in that thread."
        ),
        "matched send injects its locator quote, framed, as additionalContext"
    );

    // The matched send is terminal, so the session's open-send list is empty
    // again — the pending strip drains from server state.
    let (status, body) = get(&app, &format!("/api/sessions/{session_id}/sends")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body["sends"].as_array().expect("sends array").is_empty(),
        "a matched send no longer lists as open"
    );

    // 5. GET /api/sessions/{id}/threads exposes the main thread for the
    //    navigator, scoped to this session.
    let (status, body) = get(&app, &format!("/api/sessions/{session_id}/threads")).await;
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
    assert!(send_id > 0);

    // 6. Close the session: the pane is torn down (here a no-op, since this
    //    external session never had a live pane) but the data is kept. It still
    //    lists, now as closed.
    let (status, _) = post_json(
        &app,
        &format!("/api/sessions/{session_id}/close"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, body) = get(&app, "/api/sessions").await;
    assert_eq!(status, StatusCode::OK);
    let sessions = body["sessions"].as_array().expect("sessions array");
    assert_eq!(sessions.len(), 1, "closing keeps the session in the store");
    assert_eq!(sessions[0]["open"], false, "the session lists as closed");
    assert_eq!(
        sessions[0]["last_activity_at"], "2026-01-01T00:00:00Z",
        "last_activity_at reflects the ingested message's timestamp"
    );

    // Open/close/threads on an unknown session id are 404.
    let (status, _) = post_json(&app, "/api/sessions/ghost/open", json!({})).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "open of unknown id is 404");
    let (status, _) = post_json(&app, "/api/sessions/ghost/close", json!({})).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "close of unknown id is 404");
    let (status, _) = get(&app, "/api/sessions/ghost/threads").await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "threads of unknown id is 404"
    );
    let (status, _) = get(&app, "/api/sessions/ghost/sends").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "sends of unknown id is 404");

    let _ = std::fs::remove_file(&transcript_path);
}

/// A new-session send (`new_session: true`, no thread) spawns a fresh session
/// with the first prompt enqueued onto its `main` thread before the launch:
/// Delta mints the session id up front, so the response carries the real,
/// already-persisted session/thread/send ids. The `spawning` row stays out of
/// the session list until its first hook activates it (so the list gains the
/// row exactly when it used to: at registration).
#[tokio::test]
async fn new_session_send_spawns_and_persists_first_prompt() {
    let (app, tmux, transcript_path, _state) = build_app();

    let (status, body) = post_json(
        &app,
        "/api/sends",
        json!({ "new_session": true, "text": "kick off a new conversation" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["send"]["text"], "kick off a new conversation");
    assert_eq!(body["send"]["status"], "dispatched");
    assert!(
        body["send"]["id"].as_i64().expect("send id") > 0,
        "the send row is persisted before the spawn"
    );
    let session_id = body["send"]["session_id"]
        .as_str()
        .expect("a real session id");
    assert!(!session_id.is_empty(), "a real session id is minted");
    let thread_id = body["send"]["thread_id"].as_i64().expect("thread id");
    assert!(thread_id > 0, "the send targets the real main thread");

    // The eager row exists but stays out of the session list until its first
    // hook activates it: a message-less `spawning` session is not listable
    // (the browser could not open it, and the optimistic new-session chip
    // would mis-bind to it).
    let (status, list) = get(&app, "/api/sessions").await;
    assert_eq!(status, StatusCode::OK);
    let sessions = list["sessions"].as_array().expect("sessions array");
    assert!(
        sessions.is_empty(),
        "a message-less spawning session is not listed before its first hook"
    );
    // The open-send list is queryable for the eager row even before it is
    // listable: the browser keeps the first-send pending chip rendered from
    // server state across the spawn window.
    let (status, body) = get(&app, &format!("/api/sessions/{session_id}/sends")).await;
    assert_eq!(status, StatusCode::OK);
    let sends = body["sends"].as_array().expect("sends array");
    assert_eq!(sends.len(), 1, "the first prompt is the one open send");
    assert_eq!(sends[0]["text"], "kick off a new conversation");
    assert_eq!(sends[0]["status"], "dispatched");

    // A fresh tmux session was spawned with the first prompt carried on its
    // launch command line (claude auto-submits it at startup), not injected via
    // post-launch keystrokes.
    assert_eq!(
        tmux.created.load(Ordering::SeqCst),
        1,
        "one session spawned"
    );
    assert_eq!(
        tmux.last_command.lock().unwrap().last().map(String::as_str),
        Some("kick off a new conversation"),
        "the first prompt is the trailing positional launch argument"
    );
    assert_eq!(
        tmux.sent.load(Ordering::SeqCst),
        0,
        "the fresh spawn submits the prompt at launch, not via send_line"
    );

    let _ = std::fs::remove_file(&transcript_path);
}

/// A send that names neither a thread nor a new session is a malformed request.
#[tokio::test]
async fn send_without_a_target_is_bad_request() {
    let (app, _tmux, transcript_path, _state) = build_app();

    let (status, _) = post_json(&app, "/api/sends", json!({ "text": "no target" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let _ = std::fs::remove_file(&transcript_path);
}

#[tokio::test]
async fn create_session_endpoint_reports_starting_then_ready() {
    let (app, tmux, transcript_path, _state) = build_app();

    // No session exists yet, so the first POST /api/sessions spawns one and the
    // route serializes the lifecycle as "starting".
    let (status, body) = post_json(&app, "/api/sessions", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["status"], "starting",
        "a freshly spawned session is starting"
    );
    assert_eq!(
        tmux.created.load(Ordering::SeqCst),
        1,
        "exactly one session was spawned"
    );

    // A second POST finds a live spawn already in the registry: idempotent
    // reuse, reported as "ready" with no second spawn.
    let (status, body) = post_json(&app, "/api/sessions", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ready", "a reused session is ready");
    assert_eq!(
        tmux.created.load(Ordering::SeqCst),
        1,
        "no second session was spawned"
    );

    let _ = std::fs::remove_file(&transcript_path);
}
