//! Branching from selected text: the passage reaches the fake as hidden context
//! and the branch turn's content lands on the branch thread, not on main.

use axum::http::StatusCode;
use serde_json::json;

use crate::support::{
    build_app, drain_one_turn, get, post_json, ScenarioGuard, REPLY, REPLY_FRAGMENT,
};

/// A two-turn scenario for the branch loop: the opening turn and the branch turn
/// carry DISTINCT turn/item ids (played from the `turns` sequence, one per
/// `turn/start`), mirroring a real `codex app-server`. This is what lets the
/// branch turn's persisted messages be told apart from the opening turn's — the
/// single-turn [`streaming_turn_scenario`](crate::support::streaming_turn_scenario)
/// reuses one id set across turns, so its rows would reconcile onto each other by
/// uuid and the per-thread routing could not be observed.
fn branching_turns_scenario() -> ScenarioGuard {
    let turn = |turn_id: &str, item_id: &str| {
        format!(
            r#"{{
                "turn_id": "{turn_id}",
                "emit": [
                    {{ "type": "turn_started" }},
                    {{ "type": "item_started",   "item": {{ "id": "{item_id}", "type": "agentMessage" }} }},
                    {{ "type": "agent_message_delta", "item_id": "{item_id}", "delta": "{REPLY_FRAGMENT}" }},
                    {{ "type": "item_completed", "item": {{ "id": "{item_id}", "type": "agentMessage", "text": "{REPLY}" }} }},
                    {{ "type": "turn_completed", "status": "completed" }}
                ]
            }}"#
        )
    };
    ScenarioGuard::write(&format!(
        r#"{{
            "thread_id": "thr_branch_loop",
            "turns": [ {open}, {branch} ]
        }}"#,
        open = turn("turn_open", "item_open"),
        branch = turn("turn_branch", "item_branch"),
    ))
}

/// The Codex **branch-from-selected-text** full loop: browser → server →
/// `fake-codex`.
///
/// This is the payoff proof for Codex branch send over `thread/inject_items`.
/// After an opening turn completes, the browser sends a branch send — a
/// `thread_id` send carrying `semantic_parent_uuid` (the branched-from message)
/// and `locator_quote` (the selected passage). The stack must:
///
/// 1. Accept it (`201 CREATED`) — NOT the old `ForkCapability::None` rejection.
/// 2. Deliver the selected passage to the fake as `thread/inject_items` (hidden
///    context), which the fake records to its inject log for this assertion.
/// 3. Create the same delta-side branch structure Claude builds — a NEW thread
///    lane parented to the source thread and rooted at the branched-from
///    message (visible over `GET /api/sessions/{id}/threads`).
/// 4. Dispatch the branch turn over the same Codex send path, so it streams and
///    completes over the running event pump.
#[tokio::test(flavor = "multi_thread")]
async fn codex_branch_from_selected_text_injects_context_and_completes_over_the_full_stack() {
    const QUOTE: &str = "the selected passage to branch from";
    const PARENT_UUID: &str = "msg-branch-parent";

    let scenario = branching_turns_scenario();
    let (app, state) = build_app(&scenario);
    let mut events = state.subscribe();
    state
        .spawn_async_event_drain()
        .expect("the async drain is taken exactly once");

    // Turn 1: create a Codex session with a first prompt, and let it complete so
    // the session is idle before the branch send.
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
    let session_id = body["send"]["session_id"].as_str().unwrap().to_owned();
    let main_thread = body["send"]["thread_id"].as_i64().unwrap();
    drain_one_turn(&mut events, &session_id).await;

    // The branch send: same thread, plus the branched-from message and the
    // selected passage. Before the fix this returned an `Error::Agent`
    // rejection ("branching is not supported for a Codex session"); after it is
    // accepted and dispatches a branch turn.
    let (status, body) = post_json(
        &app,
        "/api/sends",
        json!({
            "thread_id": main_thread,
            "semantic_parent_uuid": PARENT_UUID,
            "locator_quote": QUOTE,
            "text": "branch text",
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "the Codex branch send is accepted (no ForkCapability rejection): {body:?}"
    );
    let branch_thread = body["send"]["thread_id"].as_i64().unwrap();
    assert_ne!(
        branch_thread, main_thread,
        "the branch send lands on a new thread lane, not the source thread"
    );
    assert_eq!(
        body["send"]["semantic_parent_uuid"].as_str(),
        Some(PARENT_UUID),
        "the branch send carries the branched-from message as its semantic parent"
    );
    assert_eq!(
        body["send"]["locator_quote"].as_str(),
        Some(QUOTE),
        "the branch send row persists the selected passage as its locator quote"
    );

    // (2) The fake received `thread/inject_items` with the selected passage as a
    // Responses API user message — the hidden context the model sees this turn.
    let injected = scenario.injected_items();
    assert_eq!(
        injected.len(),
        1,
        "exactly one thread/inject_items reached the fake, got {injected:?}"
    );
    let item = &injected[0][0];
    assert_eq!(
        item["type"],
        json!("message"),
        "the injected item is a message"
    );
    assert_eq!(item["role"], json!("user"), "injected as a user message");
    assert_eq!(
        item["content"][0]["type"],
        json!("input_text"),
        "the injected content is input_text"
    );
    assert_eq!(
        item["content"][0]["text"],
        json!(QUOTE),
        "the injected item carries the branched-from passage verbatim"
    );

    // (3) A new delta thread/branch exists with the right structure: parented to
    // the source thread, rooted at the branched-from message, titled from the
    // selected passage.
    let (status, body) = get(&app, &format!("/api/sessions/{session_id}/threads")).await;
    assert_eq!(status, StatusCode::OK, "threads listed: {body:?}");
    let child = body["threads"]
        .as_array()
        .expect("the threads response carries a threads array")
        .iter()
        .find(|t| t["id"].as_i64() == Some(branch_thread))
        .expect("the branch child thread is listed");
    assert_eq!(
        child["parent_thread_id"].as_i64(),
        Some(main_thread),
        "the branch child is parented to the source thread"
    );
    assert_eq!(
        child["root_message_uuid"].as_str(),
        Some(PARENT_UUID),
        "the branch child is rooted at the branched-from message"
    );
    assert_eq!(
        child["title"].as_str(),
        Some(QUOTE),
        "the branch child is titled provisionally from the selected passage"
    );

    // (4) The branch turn dispatched over the adapter: it streams and completes
    // over the same running event pump as any other Codex turn.
    let streamed = drain_one_turn(&mut events, &session_id).await;
    assert_eq!(
        streamed, REPLY_FRAGMENT,
        "the branch turn streamed its reply live before completing"
    );

    // (5) The regression this fix targets: the branch turn's persisted content
    // lands on the BRANCH thread, not main. From the live dev DB, the branch was
    // created and the `send` row routed correctly, yet `CodexConversationSource`
    // hardcoded the main thread + a null semantic parent, so the branch turn's
    // user prompt and assistant reply were written to main — leaving the branch
    // thread empty (the "no thread was created" symptom). These assertions fail
    // before the fix (the branch thread has no messages) and pass after it.
    let (status, body) = get(&app, &format!("/api/threads/{branch_thread}/messages")).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "branch thread messages fetched: {body:?}"
    );
    let branch_messages = body["messages"]
        .as_array()
        .expect("the branch thread's messages response carries a messages array");
    let branch_user = branch_messages
        .iter()
        .find(|m| m["role"] == json!("user"))
        .expect(
            "the branch turn's user prompt persisted ON THE BRANCH THREAD (empty before the fix)",
        );
    assert_eq!(
        branch_user["content_text"],
        json!("branch text"),
        "the branch user prompt carries the branch turn's text"
    );
    assert_eq!(
        branch_user["thread_id"].as_i64(),
        Some(branch_thread),
        "the branch user prompt is stored on the branch thread, not main"
    );
    assert_eq!(
        branch_user["semantic_parent_uuid"].as_str(),
        Some(PARENT_UUID),
        "the branch-ROOT user message carries the branched-from message as its \
         semantic parent, matching the send row"
    );
    let branch_assistant = branch_messages
        .iter()
        .find(|m| m["role"] == json!("assistant"))
        .expect("the branch turn's assistant reply persisted on the branch thread");
    assert_eq!(
        branch_assistant["content_text"],
        json!(REPLY),
        "the branch assistant reply persisted on the branch thread"
    );
    assert_eq!(
        branch_assistant["thread_id"].as_i64(),
        Some(branch_thread),
        "the branch assistant reply is stored on the branch thread"
    );
    assert!(
        branch_assistant["semantic_parent_uuid"].is_null(),
        "only the branch root carries the semantic parent, not the assistant reply"
    );

    // ...and the MAIN thread did NOT gain the branch turn's messages: it still
    // shows exactly turn 1's user+assistant pair. Before the fix the branch
    // turn's rows leaked onto main here.
    let (status, body) = get(&app, &format!("/api/threads/{main_thread}/messages")).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "main thread messages fetched: {body:?}"
    );
    let main_messages = body["messages"]
        .as_array()
        .expect("the main thread's messages response carries a messages array");
    assert!(
        main_messages
            .iter()
            .all(|m| m["thread_id"].as_i64() == Some(main_thread)),
        "every message on the main thread stays on main: {main_messages:?}"
    );
    assert!(
        main_messages
            .iter()
            .all(|m| m["content_text"] != json!("branch text")),
        "the branch prompt must NOT appear on the main thread: {main_messages:?}"
    );
    assert_eq!(
        main_messages
            .iter()
            .filter(|m| m["role"] == json!("user"))
            .count(),
        1,
        "main keeps only turn 1's single user prompt (the branch prompt did not \
         leak onto main): {main_messages:?}"
    );
    let main_user = main_messages
        .iter()
        .find(|m| m["role"] == json!("user"))
        .expect("turn 1's user prompt is on main");
    assert_eq!(
        main_user["content_text"],
        json!("first message"),
        "main's only user prompt is turn 1's opening message"
    );
}
