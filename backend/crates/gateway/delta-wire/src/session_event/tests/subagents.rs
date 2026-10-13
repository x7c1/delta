//! Subagents starting and finishing.

use super::*;

#[test]
fn subagent_started_serializes_with_its_correlation_and_display_fields() {
    assert_eq!(
        json(&WireSessionEvent::from(SessionEvent::SubagentStarted {
            session_id: SessionId::from("sess-1"),
            thread_id: ThreadId(2),
            tool_use_id: "toolu_01".into(),
            subagent_type: Some("general-purpose".into()),
            description: Some("Run ls and count entries".into()),
            background: false,
        })),
        serde_json::json!({
            "kind": "subagent_started",
            "session_id": "sess-1",
            "thread_id": 2,
            "tool_use_id": "toolu_01",
            "subagent_type": "general-purpose",
            "description": "Run ls and count entries",
            "background": false,
        }),
    );
}

#[test]
fn subagent_started_carries_the_background_flag_when_launched_in_background() {
    assert_eq!(
        json(&WireSessionEvent::from(SessionEvent::SubagentStarted {
            session_id: SessionId::from("sess-1"),
            thread_id: ThreadId(2),
            tool_use_id: "toolu_01".into(),
            subagent_type: Some("general-purpose".into()),
            description: Some("Long crawl".into()),
            background: true,
        })),
        serde_json::json!({
            "kind": "subagent_started",
            "session_id": "sess-1",
            "thread_id": 2,
            "tool_use_id": "toolu_01",
            "subagent_type": "general-purpose",
            "description": "Long crawl",
            "background": true,
        }),
    );
}

#[test]
fn subagent_started_keeps_null_display_fields_when_absent() {
    assert_eq!(
        json(&WireSessionEvent::from(SessionEvent::SubagentStarted {
            session_id: SessionId::from("sess-1"),
            thread_id: ThreadId(2),
            tool_use_id: "toolu_01".into(),
            subagent_type: None,
            description: None,
            background: false,
        })),
        serde_json::json!({
            "kind": "subagent_started",
            "session_id": "sess-1",
            "thread_id": 2,
            "tool_use_id": "toolu_01",
            "subagent_type": null,
            "description": null,
            "background": false,
        }),
    );
}

#[test]
fn subagent_finished_serializes_with_its_correlation_id() {
    assert_eq!(
        json(&WireSessionEvent::from(SessionEvent::SubagentFinished {
            session_id: SessionId::from("sess-1"),
            tool_use_id: "toolu_01".into(),
        })),
        serde_json::json!({
            "kind": "subagent_finished",
            "session_id": "sess-1",
            "tool_use_id": "toolu_01",
        }),
    );
}
