//! Status snapshots and their rate limits.

use super::*;

#[test]
fn status_updated_serializes_its_snapshot_with_duration_identified_rate_limits() {
    assert_eq!(
        json(&WireSessionEvent::from(SessionEvent::StatusUpdated {
            session_id: SessionId::from("sess-1"),
            snapshot: StatusSnapshot {
                model_id: Some("claude-opus-4".to_owned()),
                model_display_name: Some("Opus 4".to_owned()),
                context_used_percentage: Some(42.5),
                context_window_size: Some(200_000),
                context_current_usage: Some(85_000),
                total_input_tokens: Some(90_000),
                rate_limits: Some(vec![
                    RateLimitWindow {
                        duration_seconds: Some(5 * 60 * 60),
                        used_percentage: Some(12.0),
                        resets_at: Some(1_700_000_000),
                    },
                    RateLimitWindow {
                        duration_seconds: Some(7 * 24 * 60 * 60),
                        used_percentage: Some(3.5),
                        resets_at: Some(1_700_500_000),
                    },
                ]),
                total_cost_usd: Some(0.1234),
                current_dir: Some("/work".to_owned()),
                ..StatusSnapshot::new(AgentProvider::Claude)
            },
        })),
        serde_json::json!({
            "kind": "status_updated",
            "session_id": "sess-1",
            "snapshot": {
                "provider": "claude",
                "model_id": "claude-opus-4",
                "model_display_name": "Opus 4",
                "context_used_percentage": 42.5,
                "context_window_size": 200_000,
                "context_current_usage": 85_000,
                "total_input_tokens": 90_000,
                "rate_limits": [
                    { "duration_seconds": 18_000, "used_percentage": 12.0, "resets_at": 1_700_000_000 },
                    { "duration_seconds": 604_800, "used_percentage": 3.5, "resets_at": 1_700_500_000 },
                ],
                "total_cost_usd": 0.1234,
                "current_dir": "/work",
            },
        }),
    );
}

#[test]
fn status_updated_pre_api_snapshot_carries_nulls_and_absent_rate_limits() {
    // Before the first API response, rate limits and context usage are not
    // yet known; the snapshot still serializes with explicit nulls.
    assert_eq!(
        json(&WireSessionEvent::from(SessionEvent::StatusUpdated {
            session_id: SessionId::from("sess-1"),
            snapshot: StatusSnapshot {
                model_display_name: Some("Opus 4".to_owned()),
                ..StatusSnapshot::new(AgentProvider::Claude)
            },
        })),
        serde_json::json!({
            "kind": "status_updated",
            "session_id": "sess-1",
            "snapshot": {
                "provider": "claude",
                "model_id": null,
                "model_display_name": "Opus 4",
                "context_used_percentage": null,
                "context_window_size": null,
                "context_current_usage": null,
                "total_input_tokens": null,
                "rate_limits": null,
                "total_cost_usd": null,
                "current_dir": null,
            },
        }),
    );
}

#[test]
fn a_non_claude_snapshot_names_its_own_provider_and_may_state_only_rate_limits() {
    // The account-scoped Codex frame: it says nothing about context usage,
    // and its windows are identified by the duration the server reported.
    // The provider tag is what keeps these limits off a Claude session.
    assert_eq!(
        json(&WireSessionEvent::from(SessionEvent::StatusUpdated {
            session_id: SessionId::from("sess-1"),
            snapshot: StatusSnapshot {
                rate_limits: Some(vec![RateLimitWindow {
                    duration_seconds: Some(300 * 60),
                    used_percentage: Some(21.0),
                    resets_at: None,
                }]),
                ..StatusSnapshot::new(AgentProvider::Codex)
            },
        })),
        serde_json::json!({
            "kind": "status_updated",
            "session_id": "sess-1",
            "snapshot": {
                "provider": "codex",
                "model_id": null,
                "model_display_name": null,
                "context_used_percentage": null,
                "context_window_size": null,
                "context_current_usage": null,
                "total_input_tokens": null,
                "rate_limits": [
                    { "duration_seconds": 18_000, "used_percentage": 21.0, "resets_at": null },
                ],
                "total_cost_usd": null,
                "current_dir": null,
            },
        }),
    );
}
