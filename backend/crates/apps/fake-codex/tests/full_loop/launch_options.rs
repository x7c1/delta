//! Launch options the user registered for Codex: the ones the session selects
//! reach `thread/start` as real fields, several selected `config` rows are
//! merged into the one object that field holds (with the worktree git grant
//! unioned into it), and a selection the adapter refuses — a Delta-owned field,
//! or two `config` rows that disagree — is rejected at spawn.
//!
//! The refusal is the adapter's, but it is decided **in the request**: whether
//! the selections render onto `thread/start` depends on nothing but the request,
//! so the accept phase asks the adapter about them before it writes a row and
//! before the background launch connects. The user therefore gets a `400`
//! carrying the offending key on the send they just made, not a `spawn_failed`
//! chip about a session that was created and torn down again.

use axum::http::StatusCode;
use serde_json::json;

use crate::support::{
    build_app, drain_one_turn, get, post_json, register_launch_option, streaming_turn_scenario,
    GitRepoGuard,
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
///
/// The refusal comes from the adapter, which the accept phase consults before
/// creating anything — so it is a synchronous response and not a `spawn_failed`
/// arriving after the send was accepted.
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

    // Nothing was created: the refusal lands before the eager row is written, so
    // there is not even a row to roll back.
    let (status, sessions) = get(&app, "/api/sessions").await;
    assert_eq!(status, StatusCode::OK, "sessions fetched: {sessions:?}");
    assert_eq!(
        sessions["sessions"].as_array().map(Vec::len),
        Some(0),
        "a rejected spawn leaves no session row: {sessions:?}"
    );
}

/// Two selected `config` rows reach `thread/start` as **one** object, and the
/// worktree git grant is unioned into the writable roots one of them states.
///
/// `config` is the one `thread/start` field a launch may select twice: it is a
/// single JSON object holding many independent settings, so the shipped preset
/// and the row carrying a user's machine-specific `writable_roots` are additive,
/// not exclusive. This drives that over the whole stack — two registry rows, one
/// selection, a real git repository and a real Delta-created worktree — and
/// reads the merged object back off the params the fake app-server received.
///
/// The grant is the part only a worktree launch can prove: `<repo-root>/.git`
/// (where a linked worktree's git writes actually land) joins the list the user
/// already stated instead of being suppressed by it, so a session in a worktree
/// keeps the grant however the user configures their sandbox.
#[tokio::test(flavor = "multi_thread")]
async fn two_codex_config_options_merge_and_the_worktree_grant_joins_them() {
    const USER_ROOT: &str = "/tmp/delta-user-writable-root";
    let repo = GitRepoGuard::init("config-merge", "main");
    let scenario = streaming_turn_scenario();
    let (app, state) = build_app(&scenario);
    let mut events = state.subscribe();
    state
        .spawn_async_event_drain()
        .expect("the async drain is taken exactly once");

    let roots = register_launch_option(
        &app,
        "config",
        Some(&format!(
            r#"{{"sandbox_workspace_write.writable_roots":["{USER_ROOT}"]}}"#
        )),
        "codex",
    )
    .await;
    let reasoning = register_launch_option(
        &app,
        "config",
        Some(r#"{"model_reasoning_summary":"auto"}"#),
        "codex",
    )
    .await;

    let (status, body) = post_json(
        &app,
        "/api/sends",
        json!({
            "new_session": true,
            "provider": "codex",
            "text": "hello codex",
            "launch_option_ids": [roots, reasoning],
            "workdir": repo.path(),
            "worktree": { "start_point": { "kind": "head" } },
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "two `config` rows are merged, not rejected: {body:?}"
    );
    let session_id = body["send"]["session_id"]
        .as_str()
        .expect("the send response carries its session id")
        .to_owned();
    drain_one_turn(&mut events, &session_id).await;

    let starts = scenario.thread_starts();
    assert_eq!(starts.len(), 1, "one thread was started: {starts:?}");
    let config = &starts[0]["config"];
    assert_eq!(
        config["model_reasoning_summary"],
        json!("auto"),
        "the second row's unrelated setting survived the merge: {config}"
    );
    let granted: Vec<&str> = config["sandbox_workspace_write.writable_roots"]
        .as_array()
        .expect("the merged config states the writable roots as a list")
        .iter()
        .map(|root| root.as_str().expect("every writable root is a string"))
        .collect();
    assert_eq!(
        granted.first(),
        Some(&USER_ROOT),
        "the user's own root is first, in selection order: {granted:?}"
    );
    assert!(
        granted
            .iter()
            .any(|root| root.ends_with("/.git") && root.contains("config-merge-repo")),
        "the worktree's source repository `.git` joined the user's roots: {granted:?}"
    );
}

/// Two selected `config` rows that disagree about one setting fail the spawn:
/// `400` with the stable `launch_option_rejected` code, a message naming the
/// key path, and no session row left behind.
///
/// The merge is additive, not "last one wins" — a mis-copied duplicate has to
/// surface, because silently applying one of the two values would leave the user
/// debugging an agent that ignored a setting they can see ticked.
#[tokio::test(flavor = "multi_thread")]
async fn two_conflicting_codex_config_options_fail_the_spawn() {
    let scenario = streaming_turn_scenario();
    let (app, _state) = build_app(&scenario);

    let high = register_launch_option(
        &app,
        "config",
        Some(r#"{"model_reasoning_effort":"high"}"#),
        "codex",
    )
    .await;
    let low = register_launch_option(
        &app,
        "config",
        Some(r#"{"model_reasoning_effort":"low"}"#),
        "codex",
    )
    .await;

    let (status, body) = post_json(
        &app,
        "/api/sends",
        json!({
            "new_session": true,
            "provider": "codex",
            "text": "hello codex",
            "launch_option_ids": [high, low],
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "one setting cannot have two values: {body:?}"
    );
    assert_eq!(
        body["code"],
        json!("launch_option_rejected"),
        "the refusal carries its stable code so the browser can show the message: {body:?}"
    );
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|error| error.contains("model_reasoning_effort")),
        "the error names the conflicting key path, got {body:?}"
    );

    let (status, sessions) = get(&app, "/api/sessions").await;
    assert_eq!(status, StatusCode::OK, "sessions fetched: {sessions:?}");
    assert_eq!(
        sessions["sessions"].as_array().map(Vec::len),
        Some(0),
        "a rejected spawn leaves no session row: {sessions:?}"
    );
}
