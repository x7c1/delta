//! The multi-turn loop: a second message to an existing Codex session
//! dispatches over the bound adapter rather than Claude's resume path.

use axum::http::StatusCode;
use serde_json::json;

use crate::support::{
    build_app, drain_one_turn, get, post_json, streaming_turn_scenario, REPLY_FRAGMENT,
};

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
