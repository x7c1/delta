//! The file-change detail a card is built from: the paths, kinds and diffs an
//! approval reaches the browser with, over the full stack.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::json;
use tower::ServiceExt;

use delta_usecase::{AgentFileChange, AgentFileChangeKind, SessionEvent};

use crate::support::{build_app, get, post_json, ScenarioGuard, TIMEOUT};

/// A file-change approval reaches the browser naming the files it would change,
/// over the real stack.
///
/// The failure this exists for: `item/fileChange/requestApproval` carries only
/// `{ itemId, startedAtMs, threadId, turnId, grantRoot?, reason? }`, so every one
/// of a turn's file-change prompts reached the browser as the same truncated blob
/// of request params — interchangeable, and unanswerable on their merits. The
/// details had in fact crossed the wire a moment earlier, on the `item/started`
/// for the same item, and were thrown away because nothing correlated the two.
///
/// What this pins, end to end:
///
/// - the `permission_requested` broadcast carries the item's paths, per-change
///   kinds and diffs, plus the provider's own `reason`;
/// - the sends envelope's pending permission carries the SAME detail, so a
///   client that missed the event rebuilds the same card from a plain refetch
///   instead of one degraded to the JSON summary. The two surfaces agreeing is
///   the point: a reconnect is exactly when a user is most likely to be staring
///   at an unanswered prompt;
/// - both surfaces also carry the `grantRoot` the approval asked for. It is the
///   broadest thing the dialog grants — writes anywhere under that root for the
///   rest of the session, well past the two files the item lists — and it rides
///   the params rather than the item, so it takes a different path through the
///   pump than the detail does and is pinned here alongside it.
#[tokio::test(flavor = "multi_thread")]
async fn codex_file_change_approval_reaches_the_browser_with_its_paths_and_diff() {
    // The item states the patch; the approval that follows names only its id.
    let scenario = ScenarioGuard::write(
        r#"{
            "thread_id": "thr_fc_detail",
            "turn": {
                "turn_id": "turn_fc_detail",
                "emit": [
                    { "type": "turn_started" },
                    { "type": "item_started", "item": { "id": "fc_1", "type": "fileChange",
                      "status": "inProgress", "changes": [
                        { "path": "src/lib.rs", "kind": { "type": "update" }, "diff": "@@ -1 +1 @@\n-old\n+new" },
                        { "path": "src/added.rs", "kind": { "type": "add" }, "diff": "+fresh" }
                      ] } },
                    { "type": "request_approval", "blocking": true,
                      "method": "item/fileChange/requestApproval",
                      "params": { "itemId": "fc_1", "reason": "write access",
                                  "grantRoot": "/repo" } },
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
        json!({ "new_session": true, "provider": "codex", "text": "edit a file" }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "the send was created: {body:?}"
    );
    let session_id = body["send"]["session_id"].as_str().unwrap().to_owned();

    let deadline = tokio::time::Instant::now() + TIMEOUT;
    let (request_id, file_change, grant_root) = loop {
        let event = tokio::time::timeout_at(deadline, events.recv())
            .await
            .expect("timed out waiting for the permission request")
            .expect("the broadcast channel stayed open");
        if let SessionEvent::PermissionRequested {
            request_id,
            file_change,
            grant_root,
            ..
        } = event
        {
            break (request_id, file_change, grant_root);
        }
    };

    let detail = file_change.expect("the notice carries the file-change detail");
    assert_eq!(
        detail.changes,
        vec![
            AgentFileChange {
                path: "src/lib.rs".to_owned(),
                kind: Some(AgentFileChangeKind::Update),
                diff: "@@ -1 +1 @@\n-old\n+new".to_owned(),
            },
            AgentFileChange {
                path: "src/added.rs".to_owned(),
                kind: Some(AgentFileChangeKind::Add),
                diff: "+fresh".to_owned(),
            },
        ],
        "the broadcast names the files, how each changes, and the diff"
    );
    assert_eq!(detail.reason.as_deref(), Some("write access"));
    assert_eq!(
        grant_root.as_deref(),
        Some("/repo"),
        "the broadcast states the root the request would open up, not just its files"
    );

    // The reconnect path must agree with the event path, field for field.
    let (status, body) = get(&app, &format!("/api/sessions/{session_id}/sends")).await;
    assert_eq!(status, StatusCode::OK, "the envelope fetched: {body:?}");
    let mut permission = body["permission"].clone();
    // `tool_input` is the approval's params verbatim, and they carry a wall-clock
    // `startedAtMs`, so the text cannot be matched literally. Lift it out of the
    // comparison and assert the part the correlation turned on, rather than
    // leaving a field in place that reads as an assertion and checks nothing.
    let tool_input = permission
        .as_object_mut()
        .expect("the envelope reports the pending dialog")
        .remove("tool_input")
        .expect("the dialog carries the approval's raw params as JSON text");
    let params: serde_json::Value =
        serde_json::from_str(tool_input.as_str().expect("the params are JSON text"))
            .expect("the params parse");
    assert_eq!(
        params["itemId"], "fc_1",
        "the dialog names the item its detail was correlated from"
    );
    assert_eq!(
        permission,
        json!({
            "request_id": request_id,
            "tool_name": "file_change",
            "file_change": {
                "changes": [
                    { "path": "src/lib.rs", "kind": "update", "diff": "@@ -1 +1 @@\n-old\n+new" },
                    { "path": "src/added.rs", "kind": "add", "diff": "+fresh" },
                ],
                "reason": "write access",
            },
            "grant_root": "/repo",
        }),
        "the envelope re-seeds the same card the event raised"
    );

    // Answer so the parked turn finishes rather than leaving the fake suspended.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/permissions/{request_id}/decision"))
                .header("host", "127.0.0.1")
                .header("content-type", "application/json")
                .body(Body::from(json!({ "decision": "allow" }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}
