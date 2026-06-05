use delta_model::{ContentBlock, Message, PendingSendStatus, PermissionStatus, Role, SessionStatus};

#[test]
fn role_round_trips_through_string() {
    for role in [Role::User, Role::Assistant, Role::System, Role::Other] {
        assert_eq!(Role::parse(role.as_str()).unwrap(), role);
    }
}

#[test]
fn unknown_transcript_type_is_other_not_error() {
    assert_eq!(Role::from_transcript_type("summary"), Role::Other);
    assert_eq!(Role::from_transcript_type("assistant"), Role::Assistant);
}

#[test]
fn status_enums_round_trip() {
    for s in [
        SessionStatus::Active,
        SessionStatus::Ended,
    ] {
        assert_eq!(SessionStatus::parse(s.as_str()).unwrap(), s);
    }
    for s in [
        PendingSendStatus::Pending,
        PendingSendStatus::Matched,
        PendingSendStatus::Cancelled,
    ] {
        assert_eq!(PendingSendStatus::parse(s.as_str()).unwrap(), s);
    }
    for s in [
        PermissionStatus::Pending,
        PermissionStatus::Allowed,
        PermissionStatus::Denied,
    ] {
        assert_eq!(PermissionStatus::parse(s.as_str()).unwrap(), s);
    }
}

#[test]
fn invalid_status_is_error() {
    assert!(SessionStatus::parse("nope").is_err());
}

#[test]
fn flatten_text_joins_text_and_thinking_blocks() {
    let blocks = vec![
        ContentBlock::Thinking {
            thinking: "hmm".into(),
        },
        ContentBlock::Text {
            text: "hello".into(),
        },
        ContentBlock::ToolUse {
            id: "t1".into(),
            name: "Bash".into(),
            input: serde_json::json!({"command": "ls"}),
        },
    ];
    assert_eq!(
        Message::flatten_text(&blocks).as_deref(),
        Some("hmm\nhello")
    );
    assert_eq!(Message::flatten_text(&[]), None);
}

#[test]
fn content_block_unknown_type_parses_as_other() {
    let block: ContentBlock =
        serde_json::from_str(r#"{"type":"image","source":{"x":1}}"#).unwrap();
    assert_eq!(block, ContentBlock::Other);
}

#[test]
fn content_block_tool_result_parses() {
    let block: ContentBlock = serde_json::from_str(
        r#"{"type":"tool_result","tool_use_id":"abc","content":"done","is_error":false}"#,
    )
    .unwrap();
    match block {
        ContentBlock::ToolResult { tool_use_id, .. } => assert_eq!(tool_use_id, "abc"),
        other => panic!("unexpected: {other:?}"),
    }
}
