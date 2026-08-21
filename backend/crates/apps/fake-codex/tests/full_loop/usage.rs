//! Token accounting and account rate limits reaching the browser broadcast as
//! status snapshots, over the real stack.

use axum::http::StatusCode;
use serde_json::json;

use delta_usecase::SessionEvent;

use crate::support::{build_app, get, post_json, ScenarioGuard, TIMEOUT};

/// The usage loop: a Codex turn's token accounting and its account's rate
/// limits reach the browser broadcast as `StatusUpdated` snapshots, over the
/// same real stack.
///
/// The rate-limit half is the load-bearing one. `account/rateLimits/updated`
/// carries **no `threadId`** — this scenario emits it through the fake's
/// `account_notification` step precisely so it does not — so the transport
/// cannot demux it to a session and it takes the connection-level unrouted
/// path. Reaching this assertion therefore proves the whole chain: the drain
/// the adapter owns, the fan-out onto a live session's stream, the pump, and
/// the broadcast.
///
/// Both snapshots are observability only, which is asserted too: nothing here
/// is persisted, so the conversation is exactly the turn's own message.
#[tokio::test(flavor = "multi_thread")]
async fn codex_usage_and_account_rate_limits_reach_the_browser_broadcast() {
    let scenario = ScenarioGuard::write(
        r#"{
            "thread_id": "thr_usage",
            "turn": {
                "turn_id": "turn_usage",
                "emit": [
                    { "type": "turn_started" },
                    { "type": "item_completed", "item": { "id": "item_1", "type": "agentMessage", "text": "counted" } },
                    { "type": "notification", "method": "thread/tokenUsage/updated",
                      "params": { "turnId": "turn_usage", "tokenUsage": {
                          "total": { "totalTokens": 500000, "inputTokens": 480000, "cachedInputTokens": 400000,
                                     "outputTokens": 20000, "reasoningOutputTokens": 5000 },
                          "last": { "totalTokens": 50000, "inputTokens": 48000, "cachedInputTokens": 40000,
                                    "outputTokens": 2000, "reasoningOutputTokens": 500 },
                          "modelContextWindow": 200000 } } },
                    { "type": "account_notification", "method": "account/rateLimits/updated",
                      "params": { "rateLimits": {
                          "primary": { "usedPercent": 21, "resetsAt": 1700000000, "windowDurationMins": 300 },
                          "secondary": { "usedPercent": 4, "resetsAt": 1700500000, "windowDurationMins": 10080 },
                          "planType": "pro" } } },
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
        json!({ "new_session": true, "provider": "codex", "text": "count my tokens" }),
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

    // Collect status snapshots until both halves have been seen. They arrive on
    // independent paths (the thread demux and the connection drain), so the
    // order between them is not guaranteed and must not be asserted.
    let mut context_snapshot = None;
    let mut rate_limit_snapshot = None;
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    while context_snapshot.is_none() || rate_limit_snapshot.is_none() {
        let event = tokio::time::timeout_at(deadline, events.recv())
            .await
            .expect("timed out waiting for the Codex usage snapshots")
            .expect("the broadcast channel stayed open");
        if let SessionEvent::StatusUpdated {
            session_id: sid,
            snapshot,
        } = event
        {
            assert_eq!(sid.as_str(), session_id, "the snapshot names our session");
            assert_eq!(
                snapshot.provider,
                delta_usecase::AgentProvider::Codex,
                "a Codex snapshot says so, so the browser cannot file it under Claude"
            );
            if snapshot.context_used_percentage.is_some() {
                context_snapshot = Some(snapshot);
            } else if snapshot.rate_limits.is_some() {
                rate_limit_snapshot = Some(snapshot);
            }
        }
    }

    let context = context_snapshot.expect("a context-usage snapshot");
    assert_eq!(
        context.context_used_percentage,
        Some(25.0),
        "the last call's 50k of a 200k window, computed at the Codex edge"
    );
    assert_eq!(context.context_current_usage, Some(50_000));
    assert_eq!(
        context.rate_limits, None,
        "a token-usage frame states nothing about rate limits, so it cannot clear them"
    );

    let windows = rate_limit_snapshot
        .expect("a rate-limit snapshot")
        .rate_limits
        .expect("the account's windows");
    assert_eq!(
        windows.len(),
        2,
        "both account windows crossed the unrouted path: {windows:?}"
    );
    assert_eq!(windows[0].duration_seconds, Some(5 * 60 * 60));
    assert_eq!(windows[0].used_percentage, Some(21.0));
    assert_eq!(windows[1].duration_seconds, Some(7 * 24 * 60 * 60));
    assert_eq!(windows[1].used_percentage, Some(4.0));

    // Observability only: the usage frames persisted nothing, so the thread
    // holds exactly the turn's own prompt and reply.
    let thread_id = body["send"]["thread_id"].as_i64().unwrap();
    let (status, messages) = get(&app, &format!("/api/threads/{thread_id}/messages")).await;
    assert_eq!(status, StatusCode::OK);
    let texts: Vec<&str> = messages["messages"]
        .as_array()
        .expect("a message list")
        .iter()
        .filter_map(|message| message["content"][0]["text"].as_str())
        .collect();
    assert_eq!(
        texts,
        vec!["count my tokens", "counted"],
        "usage frames add no messages of their own"
    );
}
