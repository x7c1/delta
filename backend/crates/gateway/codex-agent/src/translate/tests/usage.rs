//! Token usage and the account's rate limits.

use super::*;

/// The token-usage breakdown the real server sends: every field of
/// `TokenUsageBreakdown` is required, so a fixture that omits one would not
/// be a shape the server can produce.
fn breakdown(total: u64, input: u64) -> Value {
    json!({
        "totalTokens": total,
        "inputTokens": input,
        "cachedInputTokens": 0,
        "outputTokens": 0,
        "reasoningOutputTokens": 0,
    })
}

#[test]
fn thread_token_usage_becomes_a_usage_event_with_a_percentage_of_the_context_window() {
    // The real `ThreadTokenUsageUpdatedNotification`: thread-scoped, so it
    // arrives through the per-thread demux and must NOT fall into the
    // catch-all that used to swallow it.
    let events = translate_notification(&notification(
        "thread/tokenUsage/updated",
        json!({
            "threadId": "thr_1",
            "turnId": "turn_1",
            "tokenUsage": {
                // The proportions a real session reaches: the running
                // `total` is already 2.5x the window while the last call —
                // the conversation actually in the window — is a quarter of
                // it.
                "total": breakdown(500_000, 480_000),
                "last": breakdown(50_000, 48_000),
                "modelContextWindow": 200_000,
            }
        }),
    ));
    assert_eq!(
        events,
        vec![AgentEvent::TokenUsageUpdated {
            usage: AgentTokenUsage {
                // 50_000 / 200_000 — computed here, at Codex's own edge,
                // because the server never sends a percentage.
                context_used_percentage: Some(25.0),
                context_window_size: Some(200_000),
                // `last`, never the running `total`: the latter would read
                // 250% here, and only ever climbs.
                context_current_usage: Some(50_000),
                // The one genuinely cumulative number, so this one is
                // `total`'s.
                total_input_tokens: Some(480_000),
            },
        }]
    );
}

#[test]
fn token_usage_without_a_context_window_reports_no_percentage() {
    // `modelContextWindow` is nullable. With nothing to divide by there is
    // no honest percentage, so the counts are still reported and the
    // percentage is omitted — never NaN, never a fabricated 0%.
    for window in [json!(null), json!(0)] {
        let events = translate_notification(&notification(
            "thread/tokenUsage/updated",
            json!({
                "threadId": "thr_1",
                "turnId": "turn_1",
                "tokenUsage": {
                    "total": breakdown(500_000, 480_000),
                    "last": breakdown(50_000, 48_000),
                    "modelContextWindow": window,
                }
            }),
        ));
        assert_eq!(
            events,
            vec![AgentEvent::TokenUsageUpdated {
                usage: AgentTokenUsage {
                    context_used_percentage: None,
                    context_window_size: None,
                    context_current_usage: Some(50_000),
                    total_input_tokens: Some(480_000),
                },
            }],
            "modelContextWindow {window} must yield no percentage"
        );
    }

    // A frame with no `tokenUsage` at all states nothing.
    assert!(translate_notification(&notification(
        "thread/tokenUsage/updated",
        json!({ "threadId": "thr_1", "turnId": "turn_1" })
    ))
    .is_empty());
}

#[test]
fn account_rate_limits_become_duration_identified_windows() {
    // The real `AccountRateLimitsUpdatedNotification`: note the absence of
    // any `threadId` — this frame belongs to the account, not a thread.
    let mut limits = AccountRateLimits::default();
    let events = limits.translate(&notification(
            "account/rateLimits/updated",
            json!({
                "rateLimits": {
                    "primary": { "usedPercent": 21, "resetsAt": 1_700_000_000_i64, "windowDurationMins": 300 },
                    "secondary": { "usedPercent": 4, "resetsAt": 1_700_500_000_i64, "windowDurationMins": 10_080 },
                    "planType": "pro",
                }
            }),
        ));
    assert_eq!(
        events,
        vec![AgentEvent::RateLimitsUpdated {
            windows: vec![
                RateLimitWindow {
                    // 300 minutes on the wire is 5 hours of neutral window.
                    duration_seconds: Some(5 * 60 * 60),
                    used_percentage: Some(21.0),
                    resets_at: Some(1_700_000_000),
                },
                RateLimitWindow {
                    duration_seconds: Some(7 * 24 * 60 * 60),
                    used_percentage: Some(4.0),
                    resets_at: Some(1_700_500_000),
                },
            ],
        }]
    );
}

#[test]
fn a_sparse_rate_limit_update_merges_and_never_clears_what_was_observed() {
    // The vendored schema is explicit: a rolling update is sparse, and a
    // null field "does not clear a previously observed value".
    let mut limits = AccountRateLimits::default();
    limits.translate(&notification(
            "account/rateLimits/updated",
            json!({
                "rateLimits": {
                    "primary": { "usedPercent": 21, "resetsAt": 1_700_000_000_i64, "windowDurationMins": 300 },
                    "secondary": { "usedPercent": 4, "resetsAt": 1_700_500_000_i64, "windowDurationMins": 10_080 },
                }
            }),
        ));

    // A second update naming only `primary` — and, within it, only the
    // required `usedPercent`. Everything it does not carry survives: the
    // whole `secondary` window, and `primary`'s own reset time and duration.
    let events = limits.translate(&notification(
        "account/rateLimits/updated",
        json!({
            "rateLimits": {
                "primary": { "usedPercent": 37, "resetsAt": null, "windowDurationMins": null },
                "secondary": null,
            }
        }),
    ));
    assert_eq!(
        events,
        vec![AgentEvent::RateLimitsUpdated {
            windows: vec![
                RateLimitWindow {
                    duration_seconds: Some(5 * 60 * 60),
                    used_percentage: Some(37.0),
                    resets_at: Some(1_700_000_000),
                },
                RateLimitWindow {
                    duration_seconds: Some(7 * 24 * 60 * 60),
                    used_percentage: Some(4.0),
                    resets_at: Some(1_700_500_000),
                },
            ],
        }]
    );

    // An update that omits both keys entirely leaves everything standing.
    let events = limits.translate(&notification(
        "account/rateLimits/updated",
        json!({ "rateLimits": { "planType": "pro" } }),
    ));
    let AgentEvent::RateLimitsUpdated { windows } = &events[0] else {
        panic!("expected a rate-limit event, got {events:?}");
    };
    assert_eq!(windows.len(), 2, "an omitted window is not a cleared one");
    assert_eq!(windows[0].used_percentage, Some(37.0));
}

#[test]
fn an_account_frame_that_states_nothing_yields_no_event() {
    let mut limits = AccountRateLimits::default();
    // No `rateLimits` object: nothing was observed, so nothing is emitted —
    // an empty window list here would wipe the display on a malformed frame.
    assert!(limits
        .translate(&notification("account/rateLimits/updated", json!({})))
        .is_empty());
    // A method this build does not model is dropped, not guessed at.
    assert!(limits
        .translate(&notification(
            "account/somethingNew",
            json!({ "whatever": true })
        ))
        .is_empty());
}

#[test]
fn the_account_frame_is_not_thread_scoped() {
    // Guard against the fake (and any future scenario) drifting into
    // stamping a `threadId` onto the account frame: the thread-scoped
    // translator must not model it, because in production it never reaches
    // that path at all.
    assert!(translate_notification(&notification(
        "account/rateLimits/updated",
        json!({ "rateLimits": { "primary": { "usedPercent": 21 } } })
    ))
    .is_empty());
}
