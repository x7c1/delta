//! Launch options the user registered for Codex: the ones the session selects
//! reach `thread/start` as real fields, and one naming a Delta-owned field is
//! rejected at spawn.

use axum::http::StatusCode;
use serde_json::json;

use crate::support::{
    build_app, drain_one_turn, get, post_json, register_launch_option, streaming_turn_scenario,
};

/// The Codex **launch-options** full loop: options the user registered for
/// Codex and selected when starting a session reach the provider as
/// `thread/start` fields.
///
/// This is the regression proof for the bug where the Settings UI happily
/// registered a Codex-scoped launch option and the new-session picker offered
/// it, but selecting it made the spawn fail outright — the core rejected any
/// selection for a non-Claude provider. The test registers three options over
/// the real REST registry, starts a Codex session selecting them, and reads
/// back the `thread/start` params the fake app-server actually received.
///
/// It also pins the value-mapping rule: a value that is not valid JSON is the
/// string it looks like, a value that parses keeps its real type, and a
/// valueless option is the bare boolean `true`.
#[tokio::test(flavor = "multi_thread")]
async fn codex_launch_options_reach_thread_start_over_the_full_stack() {
    let scenario = streaming_turn_scenario();
    let (app, state) = build_app(&scenario);
    let mut events = state.subscribe();
    state
        .spawn_async_event_drain()
        .expect("the async drain is taken exactly once");

    // A plain string value, a JSON-object value, and a valueless option.
    let model = register_launch_option(&app, "model", Some("gpt-5.6-sol"), "codex").await;
    let config = register_launch_option(
        &app,
        "config",
        Some(r#"{"tools":{"web_search":true}}"#),
        "codex",
    )
    .await;
    let ephemeral = register_launch_option(&app, "ephemeral", None, "codex").await;

    let (status, body) = post_json(
        &app,
        "/api/sends",
        json!({
            "new_session": true,
            "provider": "codex",
            "text": "hello codex",
            "launch_option_ids": [model, config, ephemeral],
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "a Codex session selecting launch options starts (it used to fail): {body:?}"
    );
    let session_id = body["send"]["session_id"]
        .as_str()
        .expect("the send response carries its session id")
        .to_owned();

    // Let the opening turn finish, so the session is unambiguously live rather
    // than merely accepted.
    drain_one_turn(&mut events, &session_id).await;

    let starts = scenario.thread_starts();
    assert_eq!(starts.len(), 1, "one thread was started: {starts:?}");
    let params = &starts[0];
    assert!(
        params["cwd"].as_str().is_some_and(|cwd| !cwd.is_empty()),
        "Delta's own cwd is still sent, got {params:?}"
    );
    assert_eq!(
        params["model"],
        json!("gpt-5.6-sol"),
        "a non-JSON value arrives as the string it looks like"
    );
    assert_eq!(
        params["config"],
        json!({ "tools": { "web_search": true } }),
        "a JSON value arrives with its real type, not as a quoted string"
    );
    assert_eq!(
        params["ephemeral"],
        json!(true),
        "a valueless option switches its boolean field on"
    );
}

/// A launch option naming a `thread/start` field Delta fills in itself is
/// rejected loudly at spawn: `400` naming the offending key, and no session row
/// left behind.
///
/// `cwd` is the field that matters — with a worktree it is the resolved
/// worktree path, and the session's repo-root / display-name /
/// branch-at-launch columns are recorded against it — so a user option
/// silently overriding it would leave those columns describing a directory the
/// agent is not running in. Failing the spawn is the only honest answer.
#[tokio::test(flavor = "multi_thread")]
async fn a_codex_launch_option_overriding_a_delta_owned_field_fails_the_spawn() {
    let scenario = streaming_turn_scenario();
    let (app, _state) = build_app(&scenario);

    let cwd = register_launch_option(&app, "cwd", Some("/somewhere/else"), "codex").await;

    let (status, body) = post_json(
        &app,
        "/api/sends",
        json!({
            "new_session": true,
            "provider": "codex",
            "text": "hello codex",
            "launch_option_ids": [cwd],
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "a Delta-owned field is rejected, not silently applied: {body:?}"
    );
    assert!(
        body["error"].as_str().is_some_and(|e| e.contains("cwd")),
        "the error names the offending key, got {body:?}"
    );

    // The eager session row was rolled back, so a rejected spawn leaves nothing
    // behind for the navigator to show.
    let (status, sessions) = get(&app, "/api/sessions").await;
    assert_eq!(status, StatusCode::OK, "sessions fetched: {sessions:?}");
    assert_eq!(
        sessions["sessions"].as_array().map(Vec::len),
        Some(0),
        "a rejected spawn leaves no session row: {sessions:?}"
    );
}
