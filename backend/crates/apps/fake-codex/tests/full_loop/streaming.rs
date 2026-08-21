//! A plain Codex turn over the whole stack: the reply streams, persists and
//! completes, and a reasoning turn's thinking never lands as reply text.

use axum::http::StatusCode;
use serde_json::{json, Value};

use delta_usecase::SessionEvent;

use crate::support::{
    build_app, drain_one_turn, get, post_json, ScenarioGuard, REPLY, REPLY_FRAGMENT, TIMEOUT,
};

#[tokio::test(flavor = "multi_thread")]
async fn codex_prompt_streams_persists_and_completes_over_the_full_stack() {
    // A scripted turn using the real item shapes: the assistant item starts
    // (announced, no text yet), a streaming `item/agentMessage/delta` carries a
    // strict prefix of the reply (→ a live `AssistantDelta`), the completed item
    // carries the full text (→ the persisted `AssistantMessage`), then a clean
    // turn completion.
    let scenario = ScenarioGuard::write(&format!(
        r#"{{
            "thread_id": "thr_full_loop",
            "turn": {{
                "turn_id": "turn_full_loop",
                "emit": [
                    {{ "type": "turn_started" }},
                    {{ "type": "item_started",   "item": {{ "id": "item_1", "type": "agentMessage" }} }},
                    {{ "type": "agent_message_delta", "item_id": "item_1", "delta": "{REPLY_FRAGMENT}" }},
                    {{ "type": "item_completed", "item": {{ "id": "item_1", "type": "agentMessage", "text": "{REPLY}" }} }},
                    {{ "type": "turn_completed", "status": "completed" }}
                ]
            }}
        }}"#
    ));

    let (app, state) = build_app(&scenario);
    // Subscribe and start the async-seam drain BEFORE the prompt, so no event the
    // pump emits after the send returns can be missed.
    let mut events = state.subscribe();
    state
        .spawn_async_event_drain()
        .expect("the async drain is taken exactly once");

    // 1. Create a Codex session with a first prompt over the REST surface.
    let (status, body) = post_json(
        &app,
        "/api/sends",
        json!({ "new_session": true, "provider": "codex", "text": "hello codex" }),
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
    let thread_id = body["send"]["thread_id"]
        .as_i64()
        .expect("the send response carries its main thread id");

    // 2. Collect the pump's broadcast events until the turn is proven streamed
    //    and completed (or the timeout trips).
    let mut streamed_reply = String::new();
    let mut turn_completed = false;
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    while !turn_completed {
        let event = tokio::time::timeout_at(deadline, events.recv())
            .await
            .expect("timed out waiting for the Codex turn to complete over the broadcast")
            .expect("the broadcast channel stayed open");
        match event {
            SessionEvent::AssistantStreaming {
                session_id: sid,
                delta,
                final_,
                ..
            } => {
                assert_eq!(sid.as_str(), session_id, "streaming names our session");
                assert!(!final_, "a Codex streaming delta is never the final chunk");
                streamed_reply.push_str(&delta);
            }
            SessionEvent::TurnCompleted {
                session_id: sid,
                thread_id: tid,
                ..
            } => {
                assert_eq!(
                    sid.as_str(),
                    session_id,
                    "turn completion names our session"
                );
                assert_eq!(
                    tid,
                    Some(delta_usecase::ThreadId(thread_id)),
                    "the completed turn is attributed to the session's main thread"
                );
                turn_completed = true;
            }
            _ => {}
        }
    }
    assert_eq!(
        streamed_reply, REPLY_FRAGMENT,
        "the assistant reply was streamed live before the turn completed"
    );

    // 3. The completed assistant message was persisted: the store-backed
    //    messages endpoint (not the live event) returns it on the main thread.
    let (status, body) = get(&app, &format!("/api/threads/{thread_id}/messages")).await;
    assert_eq!(status, StatusCode::OK, "messages fetched: {body:?}");
    let messages = body["messages"]
        .as_array()
        .expect("the messages response carries a messages array");
    let assistant = messages
        .iter()
        .find(|m| m["role"] == json!("assistant"))
        .expect("the assistant message was persisted");
    assert_eq!(
        assistant["content_text"],
        json!(REPLY),
        "the persisted assistant message carries the completed reply"
    );
    assert_eq!(
        assistant["provider_item_id"],
        json!("item_1"),
        "the persisted message keeps the provider item id as its reconcile key"
    );
    // The message time reached the persisted row: the item's `completedAtMs`
    // envelope was carried onto the neutral event and folded into `created_at`
    // as the canonical ISO-8601 UTC string (converted from `ENVELOPE_TS_MS`).
    assert_eq!(
        assistant["created_at"],
        json!("2026-07-17T07:12:18.000Z"),
        "the Codex item timestamp is persisted as an ISO-8601 created_at"
    );
    // This scenario's app-server reports no git metadata at all — the shape a
    // thread outside a git working tree gets — so the branch degrades to null
    // rather than being invented, all the way through to the persisted row.
    assert_eq!(
        assistant["git_branch"],
        Value::Null,
        "no gitInfo in the thread/start response means no branch on the message"
    );
    // The user prompt persisted too, so the loop is a real conversation.
    let user = messages
        .iter()
        .find(|m| m["role"] == json!("user"))
        .expect("the user prompt was persisted as well");
    assert!(
        user["created_at"].as_str().is_some_and(|s| !s.is_empty()),
        "the persisted user prompt also carries a non-empty created_at, got {:?}",
        user["created_at"]
    );
}

/// The Codex **reasoning** full loop: a turn whose model reasons before replying
/// must persist that reasoning as a `thinking` content block, so a Codex session
/// shows the model's thinking exactly as a Claude one does.
///
/// The scripted turn plays the real reasoning shapes: the `reasoning` item opens
/// empty, streams a summary fragment (`item/reasoning/summaryTextDelta`), and
/// completes with its `summary` parts; the assistant reply follows as its own
/// `agentMessage` item. The test asserts the reasoning landed as its own
/// `thinking` block on its own message AND — the invariant the earlier drop
/// existed to protect — that it was never mis-filed as reply text, neither in the
/// persisted assistant message nor in the live `AssistantStreaming` preview.
#[tokio::test(flavor = "multi_thread")]
async fn codex_reasoning_persists_as_a_thinking_block_and_is_never_reply_text() {
    let scenario = ScenarioGuard::write(&format!(
        r#"{{
            "thread_id": "thr_reasoning",
            "turn": {{
                "turn_id": "turn_reasoning",
                "emit": [
                    {{ "type": "turn_started" }},
                    {{ "type": "item_started",   "item": {{ "id": "reason_1", "type": "reasoning", "content": [], "summary": [] }} }},
                    {{ "type": "notification", "method": "item/reasoning/summaryTextDelta",
                       "params": {{ "itemId": "reason_1", "summaryIndex": 0, "delta": "Weighing" }} }},
                    {{ "type": "item_completed", "item": {{ "id": "reason_1", "type": "reasoning", "content": [],
                       "summary": ["Weighing the options.", "Picking the simplest."] }} }},
                    {{ "type": "item_started",   "item": {{ "id": "item_1", "type": "agentMessage" }} }},
                    {{ "type": "agent_message_delta", "item_id": "item_1", "delta": "{REPLY_FRAGMENT}" }},
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

    let (status, body) = post_json(
        &app,
        "/api/sends",
        json!({ "new_session": true, "provider": "codex", "text": "think it through" }),
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
    let thread_id = body["send"]["thread_id"]
        .as_i64()
        .expect("the send response carries its main thread id");

    let streamed_reply = drain_one_turn(&mut events, &session_id).await;
    assert_eq!(
        streamed_reply, REPLY_FRAGMENT,
        "only the reply streams live; the reasoning fragment must not reach the \
         assistant preview, which would show the model's thinking as its answer"
    );

    let (status, body) = get(&app, &format!("/api/threads/{thread_id}/messages")).await;
    assert_eq!(status, StatusCode::OK, "messages fetched: {body:?}");
    let messages = body["messages"]
        .as_array()
        .expect("the messages response carries a messages array");

    // The reasoning persisted as its own message, carrying a single `thinking`
    // block whose parts joined into one text.
    let reasoning = messages
        .iter()
        .find(|m| m["provider_item_id"] == json!("reason_1"))
        .expect("the reasoning item was persisted");
    assert_eq!(reasoning["role"], json!("assistant"));
    assert_eq!(
        reasoning["content"],
        json!([{
            "type": "thinking",
            "thinking": "Weighing the options.\n\nPicking the simplest.",
        }]),
        "the reasoning is a thinking block, not a text block"
    );

    // The reply is a separate message and carries only the reply — the reasoning
    // never leaked into it.
    let reply = messages
        .iter()
        .find(|m| m["provider_item_id"] == json!("item_1"))
        .expect("the assistant reply was persisted");
    assert_eq!(
        reply["content"],
        json!([{ "type": "text", "text": REPLY }]),
        "the reply carries only its own text"
    );
    assert!(
        !messages.iter().any(|m| {
            m["content"].as_array().is_some_and(|blocks| {
                blocks.iter().any(|b| {
                    b["type"] == json!("text")
                        && b["text"].as_str().is_some_and(|t| t.contains("Weighing"))
                })
            })
        }),
        "no persisted text block may carry the reasoning: {messages:?}"
    );
}
