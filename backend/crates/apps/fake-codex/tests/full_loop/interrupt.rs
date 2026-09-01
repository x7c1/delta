//! Interrupting an in-flight turn from the browser, over the real stack.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::json;
use tower::ServiceExt;

use delta_usecase::SessionEvent;

use crate::support::{await_session_registered, build_app, get, post_json, ScenarioGuard, TIMEOUT};

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

    // The send is *accepted* before the adapter has connected, so wait for the
    // launch to bind before interrupting: an interrupt aimed at a session whose
    // agent is not up yet is a well-defined no-op, and nothing would ever settle
    // the turn.
    await_session_registered(&mut events, &session_id).await;

    // Interrupt the in-flight turn over the REST surface.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/sessions/{session_id}/interrupt"))
                .header("host", "127.0.0.1")
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
