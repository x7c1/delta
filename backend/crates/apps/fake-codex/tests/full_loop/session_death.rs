//! An app-server that dies mid-turn: the settle it must produce, and the send
//! that follows it standing a fresh process up over the existing resume path.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::json;
use tower::ServiceExt;

use delta_sqlite::SqliteStore;
use delta_usecase::{SessionEvent, SessionStore};

use crate::support::{
    await_turn_completion, build_app, build_app_with, get, post_json, ScenarioGuard, TIMEOUT,
};

/// The **session-death** full loop: a `codex app-server` that dies mid-turn with
/// approvals outstanding must leave nothing stranded, over the same real stack.
///
/// This is the dogfooding regression. A real app-server process was killed while
/// a turn was in flight with an approval dialog on screen. Delta noticed only at
/// the transport layer (its reader hit EOF), so nothing in the session's runtime
/// moved: the turn stayed `in_flight` forever, the dialog stayed up, the user's
/// Allow failed to write (`Broken pipe`, `500`) and the retry answered `409`, and
/// the session still reported `open: true`. Indistinguishable from a hang.
///
/// The fake plays that exact sequence — two approvals, then it exits with both
/// unanswered and the turn unfinished — and this asserts the settle, end to end:
///
/// - the turn ends over the broadcast (`turn_interrupted`), so a live browser's
///   running chip clears with no reload;
/// - every dialog is settled client-visibly (`permission_resolved` per request)
///   and none is re-raised;
/// - the session reports itself closed in the session list (the UI renders a
///   closed session view-only);
/// - the sends envelope a reconnecting browser refetches agrees: idle turn, no
///   pending dialog, depth 0;
/// - no `permission_request` row is left `pending`;
/// - a decision that arrives for a stranded request answers `409` (a conflict),
///   not the `500` a write to a dead pipe produced.
#[tokio::test(flavor = "multi_thread")]
async fn codex_app_server_death_settles_the_turn_and_its_pending_approvals() {
    let scenario = ScenarioGuard::write(
        r#"{
            "thread_id": "thr_death_loop",
            "turn": {
                "turn_id": "turn_death_loop",
                "emit": [
                    { "type": "turn_started" },
                    { "type": "request_approval", "params": { "itemId": "exec_1", "command": "cat a", "cwd": "/tmp" } },
                    { "type": "request_approval", "params": { "itemId": "exec_2", "command": "cat b", "cwd": "/tmp" } },
                    { "type": "exit" },
                    { "type": "turn_completed", "status": "completed" }
                ]
            }
        }"#,
    );

    // An on-disk database, so a second store handle can prove no row is left
    // `pending` after the settle.
    let db_path = scenario.db_path();
    let (app, state) = build_app_with(SqliteStore::open(&db_path).unwrap(), &scenario);
    let mut events = state.subscribe();
    state
        .spawn_async_event_drain()
        .expect("the async drain is taken exactly once");

    let (status, body) = post_json(
        &app,
        "/api/sends",
        json!({ "new_session": true, "provider": "codex", "text": "read two files" }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "the send was created: {body:?}"
    );
    let session_id = body["send"]["session_id"].as_str().unwrap().to_owned();

    // Collect the settle off the broadcast alone — a live browser sees all of
    // this without refetching anything. The death follows the two approvals on
    // the wire, so the raises and the settles arrive on one ordered stream; the
    // session-closed signal is last.
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    let mut requested: Vec<i64> = Vec::new();
    let mut resolved: Vec<i64> = Vec::new();
    let mut turn_interrupted = false;
    let mut session_closed = false;
    while !session_closed {
        let event = tokio::time::timeout_at(deadline, events.recv())
            .await
            .expect("timed out waiting for the death to settle the session")
            .expect("the broadcast channel stayed open");
        match event {
            SessionEvent::PermissionRequested { request_id, .. } => requested.push(request_id),
            SessionEvent::PermissionResolved { request_id, .. } => resolved.push(request_id),
            SessionEvent::TurnInterrupted {
                session_id: sid, ..
            } => {
                assert_eq!(sid.as_str(), session_id, "the settle names our session");
                turn_interrupted = true;
            }
            SessionEvent::SessionClosed { session_id: sid } => {
                assert_eq!(sid.as_str(), session_id, "the close names our session");
                session_closed = true;
            }
            _ => {}
        }
    }
    assert_eq!(
        requested.len(),
        2,
        "both approvals were raised before the process died: {requested:?}"
    );
    assert!(
        turn_interrupted,
        "the stuck turn was settled over the broadcast"
    );
    assert_eq!(
        resolved, requested,
        "every raised dialog was settled client-visibly, and none was raised again \
         (the sequence would then be longer): {resolved:?} vs {requested:?}"
    );

    // The queryable truth a reconnecting browser refetches agrees with the events.
    let (status, envelope) = get(&app, &format!("/api/sessions/{session_id}/sends")).await;
    assert_eq!(status, StatusCode::OK, "sends fetched: {envelope:?}");
    assert_eq!(
        envelope["turn"]["state"],
        json!("idle"),
        "the settled session reports an idle turn: {envelope:?}"
    );
    assert_eq!(
        envelope["permission"],
        json!(null),
        "no dialog is reported: {envelope:?}"
    );
    assert_eq!(
        envelope["permission_count"],
        json!(0),
        "the pending depth is zero: {envelope:?}"
    );

    // The session list reports it closed — the state the UI renders view-only,
    // and the one the next Send resumes from.
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
        json!(false),
        "a session whose process died reports itself closed: {session:?}"
    );

    // A decision for a stranded request is a conflict, not a server error. This
    // is the exact call that returned 500 (`failed to write to app-server: Broken
    // pipe`) and then 409 in the field.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/permissions/{}/decision", requested[0]))
                .header("content-type", "application/json")
                .body(Body::from(json!({ "decision": "allow" }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::CONFLICT,
        "a decision for a request whose session died is a conflict, never a 500"
    );

    // No row was left `pending`: the guarded decide only touches a pending row,
    // so `None` here reads as "already settled".
    let probe = SqliteStore::open(&db_path).unwrap();
    for request_id in &requested {
        let still_pending = probe
            .decide_permission_request(*request_id, true)
            .await
            .unwrap();
        assert!(
            still_pending.is_none(),
            "row {request_id} was left pending after its session died"
        );
    }
}

/// Recovery after a death is the **existing resume path**, proven over the real
/// stack: Delta never respawns the dead process itself, and the next Send must
/// stand a fresh one up and run a whole turn on the same conversation.
///
/// One backend drives both halves. The first `fake-codex` dies mid-turn (the
/// scenario's `exit`); the scenario file is then rewritten, so the process the
/// resume spawns plays a clean turn — which is what a resumed real session gets:
/// a new `codex app-server` reattached to the same thread via `thread/resume`.
#[tokio::test(flavor = "multi_thread")]
async fn a_send_after_a_death_resumes_the_session_and_completes_a_fresh_turn() {
    const RESUMED_REPLY: &str = "reply after the resume";
    let scenario = ScenarioGuard::write(
        r#"{
            "thread_id": "thr_death_resume",
            "turn": {
                "turn_id": "turn_death",
                "emit": [
                    { "type": "turn_started" },
                    { "type": "exit" }
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
        json!({ "new_session": true, "provider": "codex", "text": "first message" }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "the send was created: {body:?}"
    );
    let session_id = body["send"]["session_id"].as_str().unwrap().to_owned();
    let thread_id = body["send"]["thread_id"].as_i64().unwrap();

    // The first process dies mid-turn; wait for the settle to close the session.
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    loop {
        let event = tokio::time::timeout_at(deadline, events.recv())
            .await
            .expect("timed out waiting for the death to close the session")
            .expect("the broadcast channel stayed open");
        if let SessionEvent::SessionClosed { session_id: sid } = event {
            assert_eq!(sid.as_str(), session_id);
            break;
        }
    }

    // The next process must behave, so the resumed turn can complete.
    scenario.rewrite(&format!(
        r#"{{
            "thread_id": "thr_death_resume",
            "turn": {{
                "turn_id": "turn_resumed",
                "emit": [
                    {{ "type": "turn_started" }},
                    {{ "type": "item_completed", "item": {{ "id": "item_resumed", "type": "agentMessage", "text": "{RESUMED_REPLY}" }} }},
                    {{ "type": "turn_completed", "status": "completed" }}
                ]
            }}
        }}"#
    ));

    // A Send to the settled session: the existing resume path stands a fresh
    // `fake-codex` up, reattaches to the same provider thread, and dispatches.
    let (status, body) = post_json(
        &app,
        "/api/sends",
        json!({ "thread_id": thread_id, "text": "second message" }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "a send to the settled session is accepted (it resumes): {body:?}"
    );

    // The fresh turn runs to completion over the reconnected stack, and its reply
    // is persisted on the same thread — the conversation continued.
    await_turn_completion(&mut events).await;
    let (status, body) = get(&app, &format!("/api/threads/{thread_id}/messages")).await;
    assert_eq!(status, StatusCode::OK, "messages fetched: {body:?}");
    assert!(
        body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["content_text"] == json!(RESUMED_REPLY)),
        "the resumed turn's reply persisted on the same thread: {body:?}"
    );
    let (status, sessions) = get(&app, "/api/sessions").await;
    assert_eq!(status, StatusCode::OK);
    let session = sessions["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["session"]["id"] == json!(session_id))
        .expect("our session is listed");
    assert_eq!(
        session["open"],
        json!(true),
        "the resumed session is open again: {session:?}"
    );
}
