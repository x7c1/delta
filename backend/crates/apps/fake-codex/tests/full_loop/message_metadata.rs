//! What a persisted Codex message reports about the run that produced it: the
//! model the server resolved, the branch Delta observed, and the launch dir.

use axum::http::StatusCode;
use serde_json::json;

use crate::support::{
    build_app, drain_one_turn, get, post_json, register_launch_option, GitRepoGuard, ScenarioGuard,
    REPLY,
};

/// The Codex **message-metadata** full loop: a persisted Codex message reports
/// the model the server resolved for the thread, the branch the server observed,
/// and the directory the session is running in — the feedback channel for a
/// user-selectable model.
///
/// The session selects `model=requested-by-delta` as a launch option while the
/// fake app-server answers `thread/start` with a *different* top-level `model`.
/// That divergence is the whole point: Delta's request is only one input to the
/// server's decision (the user's own `config.toml` and the server's default are
/// others), so only the response says what is actually running. Asserting the
/// **server's** value proves the metadata is read back rather than echoed.
///
/// The branch is exercised over a **real git repository** created for this test,
/// with the session started in it and **no worktree**. That combination is the
/// case the feature exists for: Delta fills the session row's `branch_at_launch`
/// only on the worktree path, and Codex's `thread/start` reports no git metadata
/// at all, so a branch on these messages can only come from Delta observing its
/// launch directory. Using a real repo (not a scripted fake) means the real
/// `Git` gateway runs, so the value is one `git` itself produced.
///
/// `cwd` is checked against the `cwd` Delta itself sent on `thread/start`, so the
/// message reports the same launch directory the agent was started in — not a
/// separately re-derived path that could drift from it.
#[tokio::test(flavor = "multi_thread")]
async fn codex_messages_report_the_resolved_model_the_observed_branch_and_the_launch_dir() {
    const RESOLVED_MODEL: &str = "gpt-5.6-sol";
    const OBSERVED_BRANCH: &str = "feature/observed-by-delta";
    let repo = GitRepoGuard::init("metadata", OBSERVED_BRANCH);
    let scenario = ScenarioGuard::write(&format!(
        r#"{{
            "thread_id": "thr_metadata",
            "model": "{RESOLVED_MODEL}",
            "turn": {{
                "turn_id": "turn_metadata",
                "emit": [
                    {{ "type": "turn_started" }},
                    {{ "type": "item_started",   "item": {{ "id": "item_1", "type": "agentMessage" }} }},
                    {{ "type": "item_completed", "item": {{ "id": "item_1", "type": "agentMessage", "text": "{REPLY}" }} }},
                    {{ "type": "turn_completed", "status": "completed" }}
                ]
            }}
        }}"#
    ));
    let (app, state) = build_app(&scenario);
    let mut events = state.subscribe();
    state
        .spawn_async_event_drain()
        .expect("the async drain is taken exactly once");

    let model_option =
        register_launch_option(&app, "model", Some("requested-by-delta"), "codex").await;
    let (status, body) = post_json(
        &app,
        "/api/sends",
        json!({
            "new_session": true,
            "provider": "codex",
            "text": "hello codex",
            "launch_option_ids": [model_option],
            // A real git repo, and NO worktree: the case Delta records no
            // branch_at_launch for.
            "workdir": repo.path(),
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "the send was created: {body:?}"
    );
    let session_id = body["send"]["session_id"].as_str().unwrap().to_owned();
    let thread_id = body["send"]["thread_id"].as_i64().unwrap();

    drain_one_turn(&mut events, &session_id).await;

    // What Delta asked for, and where it launched, as the fake actually received
    // them.
    let starts = scenario.thread_starts();
    assert_eq!(starts.len(), 1, "one thread was started: {starts:?}");
    assert_eq!(
        starts[0]["model"],
        json!("requested-by-delta"),
        "the selected launch option did ride the request"
    );
    let launch_cwd = starts[0]["cwd"]
        .as_str()
        .expect("Delta always sends its own cwd")
        .to_owned();

    let (status, body) = get(&app, &format!("/api/threads/{thread_id}/messages")).await;
    assert_eq!(status, StatusCode::OK, "messages fetched: {body:?}");
    let messages = body["messages"].as_array().unwrap();
    assert!(!messages.is_empty(), "the turn persisted messages");
    for message in messages {
        assert_eq!(
            message["model"],
            json!(RESOLVED_MODEL),
            "the persisted message reports the model the SERVER resolved, not the \
             `requested-by-delta` Delta asked for: {message:?}"
        );
        assert_eq!(
            message["cwd"],
            json!(launch_cwd),
            "the persisted message reports the directory the session launched in: {message:?}"
        );
        assert_eq!(
            message["git_branch"],
            json!(OBSERVED_BRANCH),
            "the persisted message reports the branch of its launch directory, \
             observed by Delta — this session has no worktree, so nothing else \
             knows it: {message:?}"
        );
    }
}
