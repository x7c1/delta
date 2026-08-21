//! Token accounting and the account's rate limits, as the adapter surfaces
//! them.

use agent_contract::launch_request;
use delta_usecase::{AgentAdapter, AgentEvent, SendRequest};

use crate::support::{adapter_with, collect_until, is_turn_completed, turn_scenario};

/// A turn that reports its token usage: the thread-scoped
/// `thread/tokenUsage/updated` frame must translate into a neutral usage event
/// carrying the counts AND a percentage computed from `modelContextWindow` —
/// the frame no longer dies in the translator's catch-all.
///
/// The fixture keeps a real session's proportions, where the running `total`
/// has long since passed the window (250% here) while the last call — the
/// conversation actually occupying it — is a quarter of it. That is what makes
/// the percentage below an assertion about reading `last`, not just arithmetic.
#[tokio::test]
async fn a_turns_token_usage_surfaces_with_a_percentage_of_the_context_window() {
    let (adapter, _guard) = adapter_with(&turn_scenario(
        r#"{ "type": "notification", "method": "thread/tokenUsage/updated",
             "params": { "turnId": "turn_contract", "tokenUsage": {
                 "total": { "totalTokens": 500000, "inputTokens": 480000, "cachedInputTokens": 400000,
                            "outputTokens": 20000, "reasoningOutputTokens": 5000 },
                 "last": { "totalTokens": 50000, "inputTokens": 48000, "cachedInputTokens": 40000,
                           "outputTokens": 2000, "reasoningOutputTokens": 500 },
                 "modelContextWindow": 200000 } } },"#,
    ))
    .await;
    let handle = adapter.launch(launch_request()).await.expect("launch");
    let mut stream = adapter.events(&handle);
    adapter
        .send(
            &handle,
            SendRequest {
                text: "spend some tokens".to_owned(),
            },
        )
        .await
        .expect("send");

    let events = collect_until(&mut stream, is_turn_completed).await;
    let usage = events
        .iter()
        .find_map(|event| match event {
            AgentEvent::TokenUsageUpdated { usage } => Some(usage.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no token usage surfaced; got {events:?}"));
    assert_eq!(
        usage.context_used_percentage,
        Some(25.0),
        "the last call's 50k of a 200k window is 25%, computed at the Codex edge"
    );
    assert_eq!(usage.context_window_size, Some(200_000));
    assert_eq!(usage.context_current_usage, Some(50_000));
    // The one cumulative reading, so this one comes from `total`.
    assert_eq!(usage.total_input_tokens, Some(480_000));
}

/// The same turn without a `modelContextWindow`: the counts still surface, and
/// the percentage is omitted rather than fabricated (which is what makes the
/// browser hide the bar instead of drawing a meaningless one).
#[tokio::test]
async fn token_usage_without_a_context_window_surfaces_no_percentage() {
    let (adapter, _guard) = adapter_with(&turn_scenario(
        r#"{ "type": "notification", "method": "thread/tokenUsage/updated",
             "params": { "turnId": "turn_contract", "tokenUsage": {
                 "total": { "totalTokens": 500000, "inputTokens": 480000, "cachedInputTokens": 400000,
                            "outputTokens": 20000, "reasoningOutputTokens": 5000 },
                 "last": { "totalTokens": 50000, "inputTokens": 48000, "cachedInputTokens": 40000,
                           "outputTokens": 2000, "reasoningOutputTokens": 500 },
                 "modelContextWindow": null } } },"#,
    ))
    .await;
    let handle = adapter.launch(launch_request()).await.expect("launch");
    let mut stream = adapter.events(&handle);
    adapter
        .send(
            &handle,
            SendRequest {
                text: "spend some tokens".to_owned(),
            },
        )
        .await
        .expect("send");

    let events = collect_until(&mut stream, is_turn_completed).await;
    let usage = events
        .iter()
        .find_map(|event| match event {
            AgentEvent::TokenUsageUpdated { usage } => Some(usage.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no token usage surfaced; got {events:?}"));
    assert_eq!(usage.context_used_percentage, None);
    assert_eq!(usage.context_current_usage, Some(50_000));
}

/// An `account/rateLimits/updated` emitted with **no `threadId`** — the way the
/// real server emits it — reaches the session's event stream anyway, through the
/// adapter's connection-level drain of the unrouted channel.
///
/// The `account_notification` scenario step is what makes this a real test: an
/// ordinary `notification` step would have `threadId` stamped in and would
/// exercise the per-thread demux instead, passing while the production path (no
/// thread id at all) stayed broken.
#[tokio::test]
async fn account_rate_limits_reach_the_session_without_a_thread_id() {
    let (adapter, _guard) = adapter_with(&turn_scenario(
        r#"{ "type": "account_notification", "method": "account/rateLimits/updated",
             "params": { "rateLimits": {
                 "primary": { "usedPercent": 21, "resetsAt": 1700000000, "windowDurationMins": 300 },
                 "secondary": { "usedPercent": 4, "resetsAt": 1700500000, "windowDurationMins": 10080 },
                 "planType": "pro" } } },"#,
    ))
    .await;
    let handle = adapter.launch(launch_request()).await.expect("launch");
    let mut stream = adapter.events(&handle);
    adapter
        .send(
            &handle,
            SendRequest {
                text: "trigger the account update".to_owned(),
            },
        )
        .await
        .expect("send");

    // The account frame arrives on a different task from the turn's own frames,
    // so it may land after `turn/completed`; collect until it shows up.
    let events = collect_until(&mut stream, |event| {
        matches!(event, AgentEvent::RateLimitsUpdated { .. })
    })
    .await;
    let windows = events
        .iter()
        .find_map(|event| match event {
            AgentEvent::RateLimitsUpdated { windows } => Some(windows.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no rate limits surfaced; got {events:?}"));
    assert_eq!(windows.len(), 2, "both windows surfaced: {windows:?}");
    // Windows are identified by duration, not by the server's `primary` /
    // `secondary` names: 300 minutes is a 5-hour window, 10080 a 7-day one.
    assert_eq!(windows[0].duration_seconds, Some(5 * 60 * 60));
    assert_eq!(windows[0].used_percentage, Some(21.0));
    assert_eq!(windows[0].resets_at, Some(1_700_000_000));
    assert_eq!(windows[1].duration_seconds, Some(7 * 24 * 60 * 60));
    assert_eq!(windows[1].used_percentage, Some(4.0));
}
