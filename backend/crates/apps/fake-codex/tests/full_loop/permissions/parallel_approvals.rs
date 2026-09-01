//! A parallel fan-out of approvals gating one turn, over the same real stack.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::json;
use tower::ServiceExt;

use delta_sqlite::SqliteStore;
use delta_usecase::{SessionEvent, SessionStore};

use crate::support::{
    await_turn_completion, build_app_with, get, post_json, ScenarioGuard, TIMEOUT,
};

/// A parallel approval fan-out, over the same real stack: one turn raises THREE
/// approvals before any of them is answered, and the turn completes only once
/// every one has been answered from the browser.
///
/// This is the regression the field failure demands. A real `codex app-server`
/// turn fanned 12 escalated `exec_command` calls out at once; Delta mirrored the
/// pending dialog in a single slot, so each new request overwrote the previous
/// one and only the last was answerable. The user's one Allow answered request
/// 12, the other 11 waited forever, the turn stayed in flight, and the envelope's
/// `permission` went back to `null` — "Codex never responds".
///
/// What this pins, end to end:
///
/// - all three requests survive as answerable rows (none is overwritten);
/// - the sends envelope reports the queue HEAD (the oldest, not the last writer)
///   plus the depth, so a reconnecting browser rebuilds the dialog and its
///   "N pending" indication from one refetch;
/// - answering the head promotes the next AND re-broadcasts it as a
///   `permission_requested`, so an event-only client is never left dialog-less;
/// - a Deny for one of the three declines only that request on the wire — the
///   others stay pending and answerable, and the turn still waits for them;
/// - the turn completes once all three are answered, with a decision echo per
///   request carrying the value the user chose (proof each answer reached the
///   provider), and no row is left `pending`.
#[tokio::test(flavor = "multi_thread")]
async fn codex_parallel_approvals_are_all_answerable_and_gate_the_turn() {
    // Three approvals emitted back to back (non-blocking, so all three are
    // outstanding at once), then the turn parks on `await_approvals`: it can only
    // complete after every one has an answer.
    let scenario = ScenarioGuard::write(
        r#"{
            "thread_id": "thr_parallel_perm",
            "turn": {
                "turn_id": "turn_parallel_perm",
                "emit": [
                    { "type": "turn_started" },
                    { "type": "request_approval", "params": { "itemId": "exec_1", "command": "cat a", "cwd": "/tmp" } },
                    { "type": "request_approval", "params": { "itemId": "exec_2", "command": "cat b", "cwd": "/tmp" } },
                    { "type": "request_approval", "params": { "itemId": "exec_3", "command": "cat c", "cwd": "/tmp" } },
                    { "type": "await_approvals" },
                    { "type": "turn_completed", "status": "completed" }
                ]
            }
        }"#,
    );

    // An on-disk database: the test keeps a second store handle to prove no row
    // is left `pending` at the end.
    let db_path = scenario.db_path();
    let (app, state) = build_app_with(SqliteStore::open(&db_path).unwrap(), &scenario);
    let mut events = state.subscribe();
    state
        .spawn_async_event_drain()
        .expect("the async drain is taken exactly once");

    let (status, body) = post_json(
        &app,
        "/api/sends",
        json!({ "new_session": true, "provider": "codex", "text": "read three files" }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "the send was created: {body:?}"
    );
    let session_id = body["send"]["session_id"].as_str().unwrap().to_owned();
    let thread_id = body["send"]["thread_id"].as_i64().unwrap();

    // Collect all three notices. They arrive before any answer, so nothing can
    // have been serialized by a blocked hook — this is the parallel case.
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    let mut requested: Vec<i64> = Vec::new();
    while requested.len() < 3 {
        let event = tokio::time::timeout_at(deadline, events.recv())
            .await
            .expect("timed out waiting for the three approval requests")
            .expect("the broadcast channel stayed open");
        if let SessionEvent::PermissionRequested {
            session_id: sid,
            request_id,
            tool_name,
            ..
        } = event
        {
            assert_eq!(sid.as_str(), session_id, "the notice names our session");
            // A command-execution approval names its command as the tool.
            assert!(
                ["cat a", "cat b", "cat c"].contains(&tool_name.as_str()),
                "each notice carries its own command, not a shared one: {tool_name}"
            );
            assert!(request_id > 0, "the notice carries a Delta row id");
            requested.push(request_id);
        }
    }
    assert_eq!(
        requested.len(),
        requested
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        "three DISTINCT rows: {requested:?}"
    );

    // The envelope reports the head plus the depth — the queryable state a
    // reconnecting browser rebuilds from.
    let (status, envelope) = get(&app, &format!("/api/sessions/{session_id}/sends")).await;
    assert_eq!(status, StatusCode::OK, "sends fetched: {envelope:?}");
    assert_eq!(
        envelope["permission"]["request_id"],
        json!(requested[0]),
        "the head is the OLDEST request, not the last writer: {envelope:?}"
    );
    assert_eq!(
        envelope["permission_count"],
        json!(3),
        "all three are reported pending: {envelope:?}"
    );

    // Answer them front to back, exactly as the browser walks the queue — and
    // DENY the middle one. A denial is a resolution like any other: it retires
    // only its own request, so the one behind it still promotes, stays
    // answerable, and still gates the turn.
    //
    // The turn's completion is tracked from inside the settle loop, exactly like
    // the single-approval tests in `decision_matrix` do: `PermissionResolved`
    // rides the resolver's path (the adapter emits it after writing the decision
    // to the wire) while `turn/completed` rides the provider's push path, so on a
    // loaded machine the final answer's completion can legally overtake its own
    // `PermissionResolved` on the broadcast. A separate wait that starts only
    // after the settle loop would have already consumed the completion in its
    // catch-all arm and then time out.
    let mut turn_completed = false;
    for (answered, request_id) in requested.iter().enumerate() {
        let decision = if answered == 1 { "deny" } else { "allow" };
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/permissions/{request_id}/decision"))
                    .header("host", "127.0.0.1")
                    .header(
                        "authorization",
                        format!("Bearer {}", crate::support::AUTH_TOKEN),
                    )
                    .header("content-type", "application/json")
                    .body(Body::from(json!({ "decision": decision }).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::NO_CONTENT,
            "request {request_id} was still answerable ({decision}); none was overwritten"
        );

        // The resolution settles and, while the queue still holds requests, the
        // promoted head is raised again — the invariant that failed in the field,
        // where the browser was left with no dialog while 11 requests waited.
        let expected_head = requested.get(answered + 1).copied();
        let mut resolved = false;
        let mut promoted = None;
        while !resolved || (expected_head.is_some() && promoted.is_none()) {
            let event = tokio::time::timeout_at(deadline, events.recv())
                .await
                .expect("timed out waiting for the decision to settle")
                .expect("the broadcast channel stayed open");
            match event {
                SessionEvent::PermissionResolved {
                    request_id: rid, ..
                } if rid == *request_id => resolved = true,
                SessionEvent::PermissionRequested {
                    request_id: rid, ..
                } => promoted = Some(rid),
                SessionEvent::TurnCompleted { .. } => turn_completed = true,
                _ => {}
            }
        }
        assert_eq!(
            promoted, expected_head,
            "answering the head raises the next queued dialog (none left on the last answer)"
        );

        // The envelope agrees: the head advanced and the depth shrank.
        let (_, envelope) = get(&app, &format!("/api/sessions/{session_id}/sends")).await;
        match expected_head {
            Some(head) => {
                assert_eq!(envelope["permission"]["request_id"], json!(head));
                assert_eq!(envelope["permission_count"], json!(3 - answered - 1));
            }
            None => {
                assert_eq!(envelope["permission"], json!(null));
                assert_eq!(envelope["permission_count"], json!(0));
            }
        }
    }

    // With every approval answered the fake plays the parked remainder: the turn
    // completes. A single unanswered request would hang here — which is exactly
    // what the user experienced. (Skipped when the settle loop already observed
    // the completion — see the reorder note above the loop.)
    if !turn_completed {
        await_turn_completion(&mut events).await;
    }

    // Each answer reached the provider, carrying the value the user chose: the
    // fake echoes every decision it received as an assistant message, so two
    // `accept`s plus one `decline` means three responses on the wire — no
    // orphaned request, and the denial declined exactly one of them.
    let (status, body) = get(&app, &format!("/api/threads/{thread_id}/messages")).await;
    assert_eq!(status, StatusCode::OK, "messages fetched: {body:?}");
    let echoes = |text: &str| {
        body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|m| m["role"] == json!("assistant") && m["content_text"] == json!(text))
            .count()
    };
    assert_eq!(
        echoes("accept"),
        2,
        "the two allowed approvals reached the provider: {body:?}"
    );
    assert_eq!(
        echoes("decline"),
        1,
        "the denied approval reached the provider as a decline, alone: {body:?}"
    );

    // No row is left `pending`. The guarded decide only touches a pending row, so
    // a `None` here is a read: nothing is still awaiting an answer. (The field
    // failure left 11 rows pending forever.)
    let probe = SqliteStore::open(&db_path).unwrap();
    for request_id in &requested {
        let still_pending = probe
            .decide_permission_request(*request_id, true)
            .await
            .unwrap();
        assert!(
            still_pending.is_none(),
            "row {request_id} was left pending after the turn completed"
        );
    }
}
