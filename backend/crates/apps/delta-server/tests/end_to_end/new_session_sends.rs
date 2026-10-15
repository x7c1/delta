//! Sends that start a new session, and sends that name no target at all.

use std::sync::atomic::Ordering;

use axum::http::StatusCode;
use serde_json::json;

use crate::support::{build_app, get, post_json};

/// The directory the new sessions here are started in. The end-to-end
/// workspace resolves any path as-is, so it need not exist.
const WORKDIR: &str = "/tmp/delta-e2e-workdir";

/// A new-session send (`new_session: true`, no thread) spawns a fresh session
/// with the first prompt enqueued onto its `main` thread before the launch:
/// Delta mints the session id up front, so the response carries the real,
/// already-persisted session/thread/send ids — and the `spawning` row is listed
/// straight away, so the browser can focus the new session the moment its first
/// send is accepted rather than waiting for the launch's first hook.
#[tokio::test]
async fn new_session_send_spawns_and_persists_first_prompt() {
    let (app, tmux, transcript_path, _state) = build_app();

    let (status, body) = post_json(
        &app,
        "/api/sends",
        json!({
            "new_session": true,
            "text": "kick off a new conversation",
            "workdir": WORKDIR,
        }),
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

    // The eager row is listed immediately, before any hook, carrying its
    // `spawning` status — that is what lets the browser focus the new session
    // on acceptance. It is not yet `open`: no pane is bound to it, so nothing
    // may attach to (or dispatch into) it until its first hook registers it.
    let (status, list) = get(&app, "/api/sessions").await;
    assert_eq!(status, StatusCode::OK);
    let sessions = list["sessions"].as_array().expect("sessions array");
    assert_eq!(
        sessions.len(),
        1,
        "the session is listed from the moment its first send is accepted"
    );
    assert_eq!(sessions[0]["session"]["id"], session_id);
    assert_eq!(sessions[0]["session"]["status"], "spawning");
    assert_eq!(sessions[0]["open"], false);
    assert!(
        sessions[0]["last_activity_at"].is_null(),
        "a session that has ingested nothing has no activity yet"
    );
    assert!(
        sessions[0]["session"]["pull_request_number"].is_null(),
        "a new-session send with no PR origin records none"
    );

    // Nothing can be *dispatched* into it while it is starting — no pane is
    // bound and its transcript does not exist — but a plain send is still
    // accepted: it is recorded `queued` and typed once the launch binds, so the
    // user never waits at a disabled composer through a slow checkout.
    let (status, queued) = post_json(
        &app,
        "/api/sends",
        json!({ "thread_id": thread_id, "text": "and one more while it starts" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(queued["send"]["status"], "queued");
    assert_eq!(queued["send"]["session_id"], session_id);

    // A *branch* send is the one shape still refused there: the session has
    // ingested no message, so there is nothing to branch from.
    let (status, refused) = post_json(
        &app,
        "/api/sends",
        json!({
            "thread_id": thread_id,
            "text": "branch from nothing",
            "semantic_parent_uuid": "no-such-message",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(refused["code"], "session_spawning");

    // The open-send list is queryable for the eager rows: the browser keeps the
    // pending strip rendered from server state across the spawn window — the
    // dispatched first prompt, then the queued follow-up, and nothing from the
    // refused branch send.
    let (status, body) = get(&app, &format!("/api/sessions/{session_id}/sends")).await;
    assert_eq!(status, StatusCode::OK);
    let sends = body["sends"].as_array().expect("sends array");
    assert_eq!(sends.len(), 2, "the first prompt and the queued follow-up");
    assert_eq!(sends[0]["text"], "kick off a new conversation");
    assert_eq!(sends[0]["status"], "dispatched");
    assert_eq!(sends[1]["text"], "and one more while it starts");
    assert_eq!(sends[1]["status"], "queued");

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

/// A new-session send carrying `pull_request_number` records it on the session,
/// and `GET /api/sessions` serves it **while the row is still `spawning`** — the
/// navigator card shows the PR from the moment the session is listed — and
/// still after the launch's first hook registers the session, because the
/// number is a spawn-time snapshot the activate path never touches.
#[tokio::test]
async fn new_session_send_records_the_pull_request_it_was_opened_from() {
    let (app, _tmux, transcript_path, _state) = build_app();
    let transcript_str = transcript_path.to_str().unwrap().to_owned();

    let (status, body) = post_json(
        &app,
        "/api/sends",
        json!({
            "new_session": true,
            "text": "resume PR work",
            "workdir": WORKDIR,
            "pull_request_number": 138,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let session_id = body["send"]["session_id"]
        .as_str()
        .expect("a real session id")
        .to_owned();

    let (status, list) = get(&app, "/api/sessions").await;
    assert_eq!(status, StatusCode::OK);
    let sessions = list["sessions"].as_array().expect("sessions array");
    assert_eq!(sessions[0]["session"]["status"], "spawning");
    assert_eq!(
        sessions[0]["session"]["pull_request_number"], 138,
        "the card can show the PR before the launch has bound"
    );

    // The launch's first hook activates the row. That path rewrites `cwd` and
    // `transcript_path`; the PR snapshot must survive it untouched.
    let (status, _) = post_json(
        &app,
        "/hooks/user-prompt-submit",
        json!({
            "prompt": "resume PR work",
            "session_id": session_id,
            "transcript_path": transcript_str,
            "cwd": "/work/delta",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, list) = get(&app, "/api/sessions").await;
    assert_eq!(status, StatusCode::OK);
    let sessions = list["sessions"].as_array().expect("sessions array");
    assert_eq!(sessions[0]["session"]["status"], "active");
    assert_eq!(
        sessions[0]["session"]["pull_request_number"], 138,
        "registering the session must not clear its PR origin"
    );

    let _ = std::fs::remove_file(&transcript_path);
}

/// Pull requests are numbered from 1, so a zero or negative `pull_request_number`
/// is a malformed request rather than "no PR": it is refused before anything is
/// spawned.
#[tokio::test]
async fn new_session_send_with_a_non_positive_pull_request_number_is_bad_request() {
    let (app, tmux, transcript_path, _state) = build_app();

    let (status, _) = post_json(
        &app,
        "/api/sends",
        json!({
            "new_session": true,
            "text": "go",
            "workdir": WORKDIR,
            "pull_request_number": 0,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        tmux.created.load(Ordering::SeqCst),
        0,
        "a rejected request spawns nothing"
    );

    let _ = std::fs::remove_file(&transcript_path);
}

/// A new-session send that names no working directory — neither a directory
/// nor a repository to branch a worktree off — is refused before anything is
/// written or launched: no session row is listed and no pane is created.
#[tokio::test]
async fn new_session_send_without_a_workdir_is_bad_request() {
    let (app, tmux, transcript_path, _state) = build_app();

    for body in [
        json!({ "new_session": true, "text": "go" }),
        json!({
            "new_session": true,
            "text": "go",
            "worktree": { "start_point": { "kind": "head" } },
        }),
    ] {
        let (status, refused) = post_json(&app, "/api/sends", body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            refused["error"]
                .as_str()
                .is_some_and(|message| message.contains("working directory")),
            "the refusal says a working directory is required: {refused}"
        );
    }

    assert_eq!(
        tmux.created.load(Ordering::SeqCst),
        0,
        "a refused request launches no pane"
    );
    let (status, list) = get(&app, "/api/sessions").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        list["sessions"].as_array().map(Vec::len),
        Some(0),
        "a refused request writes no session row"
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
