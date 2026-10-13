//! Item lifecycle notifications and their streaming deltas.

use super::*;

#[test]
fn agent_message_completed_is_an_assistant_message() {
    // The real `AgentMessageThreadItem` shape: `type: "agentMessage"`, the
    // reply under `text`, keyed by `id`.
    let events = translate_notification(&notification(
        "item/completed",
        json!({ "item": { "id": "item_1", "type": "agentMessage", "text": "hi", "phase": "final_answer" } }),
    ));
    assert_eq!(
        events,
        vec![AgentEvent::AssistantMessage {
            provider_item_id: "item_1".to_owned(),
            text: "hi".to_owned(),
            // This notification carries no `completedAtMs`, so `at_ms`
            // degrades to `None`.
            at_ms: None,
        }]
    );
}

#[test]
fn agent_message_started_with_text_is_a_delta_and_empty_is_nothing() {
    let with_text = translate_notification(&notification(
        "item/started",
        json!({ "item": { "id": "i1", "type": "agentMessage", "text": "partial" } }),
    ));
    assert_eq!(
        with_text,
        vec![AgentEvent::AssistantDelta {
            provider_item_id: "i1".to_owned(),
            text: "partial".to_owned(),
        }]
    );

    // A started `agentMessage` announcing the item before any text (the real
    // server streams the body via `item/agentMessage/delta`) emits nothing.
    let empty = translate_notification(&notification(
        "item/started",
        json!({ "item": { "id": "i1", "type": "agentMessage" } }),
    ));
    assert!(empty.is_empty(), "an empty started message emits nothing");
}

#[test]
fn an_agent_message_delta_notification_is_an_assistant_delta() {
    // The real `AgentMessageDeltaNotification`: the fragment under `delta`,
    // the item it extends under `itemId`.
    let events = translate_notification(&notification(
        "item/agentMessage/delta",
        json!({ "threadId": "thr_1", "turnId": "turn_1", "itemId": "i1", "delta": "chunk" }),
    ));
    assert_eq!(
        events,
        vec![AgentEvent::AssistantDelta {
            provider_item_id: "i1".to_owned(),
            text: "chunk".to_owned(),
        }]
    );
    // An empty delta emits nothing.
    assert!(translate_notification(&notification(
        "item/agentMessage/delta",
        json!({ "itemId": "i1", "delta": "" })
    ))
    .is_empty());
}

#[test]
fn a_command_execution_item_is_a_tool_call_carrying_its_real_fields() {
    // The real `CommandExecutionThreadItem` shape.
    let started = translate_notification(&notification(
        "item/started",
        json!({ "item": {
                "id": "t1", "type": "commandExecution",
                "command": "ls", "cwd": "/tmp", "status": "inProgress", "commandActions": []
            } }),
    ));
    assert_eq!(
        started,
        vec![AgentEvent::ToolStarted {
            provider_item_id: "t1".to_owned(),
            name: "command_execution".to_owned(),
            input_json: json!({
                "id": "t1", "type": "commandExecution",
                "command": "ls", "cwd": "/tmp", "status": "inProgress", "commandActions": []
            }),
            at_ms: None,
        }],
        "the whole item rides input_json so every real field is preserved"
    );

    let completed = translate_notification(&notification(
        "item/completed",
        json!({ "item": {
                "id": "t1", "type": "commandExecution",
                "command": "ls", "cwd": "/tmp", "status": "completed",
                "commandActions": [], "aggregatedOutput": "a\nb", "exitCode": 0, "durationMs": 5
            } }),
    ));
    match &completed[..] {
        [AgentEvent::ToolCompleted {
            provider_item_id,
            output_json,
            at_ms: _,
        }] => {
            assert_eq!(provider_item_id, "t1");
            assert_eq!(output_json["exitCode"], 0);
            assert_eq!(output_json["aggregatedOutput"], "a\nb");
            assert_eq!(output_json["status"], "completed");
        }
        other => panic!("expected one ToolCompleted, got {other:?}"),
    }
}

#[test]
fn a_file_change_item_is_a_tool_call() {
    // The real `FileChangeThreadItem` shape.
    let started = translate_notification(&notification(
        "item/started",
        json!({ "item": {
                "id": "fc1", "type": "fileChange", "status": "inProgress",
                "changes": [{ "path": "/x", "kind": "add" }]
            } }),
    ));
    assert_eq!(
        started,
        vec![AgentEvent::ToolStarted {
            provider_item_id: "fc1".to_owned(),
            name: "file_change".to_owned(),
            input_json: json!({
                "id": "fc1", "type": "fileChange", "status": "inProgress",
                "changes": [{ "path": "/x", "kind": "add" }]
            }),
            at_ms: None,
        }]
    );
}

#[test]
fn item_lifecycle_timestamps_populate_the_events_at_ms() {
    // `item/started` carries `startedAtMs` and `item/completed` carries
    // `completedAtMs` as siblings of `item`; each is threaded onto the
    // projected event's neutral `at_ms`.
    let started = translate_notification(&notification(
        "item/started",
        json!({
            "threadId": "thr_1", "turnId": "turn_1", "startedAtMs": 1_700_000_000_123_i64,
            "item": { "id": "t1", "type": "commandExecution", "command": "ls", "status": "inProgress" }
        }),
    ));
    assert_eq!(
        started,
        vec![AgentEvent::ToolStarted {
            provider_item_id: "t1".to_owned(),
            name: "command_execution".to_owned(),
            input_json: json!({ "id": "t1", "type": "commandExecution", "command": "ls", "status": "inProgress" }),
            at_ms: Some(1_700_000_000_123),
        }]
    );

    let completed = translate_notification(&notification(
        "item/completed",
        json!({
            "threadId": "thr_1", "turnId": "turn_1", "completedAtMs": 1_700_000_005_456_i64,
            "item": { "id": "m1", "type": "agentMessage", "text": "done" }
        }),
    ));
    assert_eq!(
        completed,
        vec![AgentEvent::AssistantMessage {
            provider_item_id: "m1".to_owned(),
            text: "done".to_owned(),
            at_ms: Some(1_700_000_005_456),
        }]
    );
}

#[test]
fn a_user_message_item_is_dropped_to_avoid_double_emitting_the_prompt() {
    // The prompt is already surfaced as `UserPromptAccepted` at send time, so
    // the echoed `UserMessageThreadItem` must not re-emit it.
    assert!(translate_notification(&notification(
            "item/started",
            json!({ "item": { "id": "u1", "type": "userMessage", "content": [{ "type": "text", "text": "hi" }] } })
        ))
        .is_empty());
    assert!(translate_notification(&notification(
            "item/completed",
            json!({ "item": { "id": "u1", "type": "userMessage", "content": [{ "type": "text", "text": "hi" }] } })
        ))
        .is_empty());
}

#[test]
fn a_reasoning_item_and_its_deltas_become_thinking_not_misfiled() {
    // A reasoning item announced before any reasoning arrived has nothing to
    // show yet, so it emits nothing rather than an empty thinking block.
    assert!(translate_notification(&notification(
        "item/started",
        json!({ "item": { "id": "r1", "type": "reasoning", "summary": [], "content": [] } })
    ))
    .is_empty());

    // The completed item becomes a thinking-bearing event — never an
    // assistant message, which would misrepresent the model's internal
    // reasoning as its reply text.
    let completed = translate_notification(&notification(
        "item/completed",
        json!({
            "completedAtMs": 1_700_000_000_123_i64,
            "item": { "id": "r1", "type": "reasoning", "summary": ["s"], "content": ["c"] }
        }),
    ));
    assert_eq!(
        completed,
        vec![AgentEvent::ThinkingMessage {
            provider_item_id: "r1".to_owned(),
            text: "c".to_owned(),
            at_ms: Some(1_700_000_000_123),
        }]
    );

    // Both text-bearing reasoning deltas are streaming thinking fragments.
    for method in [
        "item/reasoning/textDelta",
        "item/reasoning/summaryTextDelta",
    ] {
        assert_eq!(
            translate_notification(&notification(
                method,
                json!({ "itemId": "r1", "delta": "thinking", "contentIndex": 0 })
            )),
            vec![AgentEvent::ThinkingDelta {
                provider_item_id: "r1".to_owned(),
                text: "thinking".to_owned(),
            }],
            "{method} must become a thinking fragment"
        );
    }
    // The part-added boundary carries no text of its own, so it emits
    // nothing; an empty fragment does not either.
    assert!(translate_notification(&notification(
        "item/reasoning/summaryPartAdded",
        json!({ "itemId": "r1", "summaryIndex": 1 })
    ))
    .is_empty());
    assert!(translate_notification(&notification(
        "item/reasoning/textDelta",
        json!({ "itemId": "r1", "delta": "", "contentIndex": 0 })
    ))
    .is_empty());

    // Nothing on the reasoning path is ever an assistant message or a tool.
    assert!(
        !completed.iter().any(|e| matches!(
            e,
            AgentEvent::AssistantMessage { .. }
                | AgentEvent::AssistantDelta { .. }
                | AgentEvent::ToolStarted { .. }
                | AgentEvent::ToolCompleted { .. }
        )),
        "reasoning must never be mis-filed: {completed:?}"
    );
}

#[test]
fn reasoning_prefers_raw_content_and_falls_back_to_the_summary() {
    let thinking_of = |item: Value| match translate_notification(&notification(
        "item/completed",
        json!({ "item": item }),
    ))
    .as_slice()
    {
        [AgentEvent::ThinkingMessage { text, .. }] => Some(text.clone()),
        [] => None,
        other => panic!("expected one thinking message, got {other:?}"),
    };

    // Raw reasoning wins when present: the summary condenses the same
    // reasoning, so showing both would show it twice.
    assert_eq!(
        thinking_of(json!({
            "id": "r1", "type": "reasoning",
            "content": ["raw one", "raw two"], "summary": ["condensed"]
        })),
        Some("raw one\n\nraw two".to_owned()),
        "parts join as separate paragraphs"
    );
    // Summary-only is the usual case for a hosted reasoning model, which
    // withholds its raw chain-of-thought — the fallback is what keeps the
    // thinking block non-empty in practice.
    assert_eq!(
        thinking_of(json!({
            "id": "r1", "type": "reasoning", "content": [], "summary": ["a", "b"]
        })),
        Some("a\n\nb".to_owned())
    );
    // Absent fields (both default to `[]` in the schema) and blank parts
    // degrade to nothing rather than an empty thinking block.
    assert_eq!(
        thinking_of(json!({ "id": "r1", "type": "reasoning" })),
        None
    );
    assert_eq!(
        thinking_of(json!({
            "id": "r1", "type": "reasoning", "content": ["", ""], "summary": [""]
        })),
        None
    );
    // A non-string part is skipped, never rendered as JSON.
    assert_eq!(
        thinking_of(json!({
            "id": "r1", "type": "reasoning", "summary": [{ "text": "x" }, "kept"]
        })),
        Some("kept".to_owned())
    );
}

#[test]
fn an_unknown_item_type_is_skipped_not_treated_as_a_tool() {
    // A type this build does not model (e.g. `mcpToolCall`, `plan`) is a safe
    // skip — never mis-filed as a tool call, never a panic.
    for item_type in ["mcpToolCall", "plan", "webSearch", "somethingBrandNew"] {
        let started = translate_notification(&notification(
            "item/started",
            json!({ "item": { "id": "x1", "type": item_type, "status": "inProgress" } }),
        ));
        assert!(
            started.is_empty(),
            "an unknown item type `{item_type}` must not become a tool: {started:?}"
        );
    }
}

#[test]
fn unmodeled_item_deltas_are_dropped() {
    for method in [
        "item/plan/delta",
        "item/commandExecution/outputDelta",
        "item/mcpToolCall/progress",
    ] {
        assert!(
            translate_notification(&notification(
                method,
                json!({ "itemId": "x", "delta": "y" })
            ))
            .is_empty(),
            "{method} must be dropped"
        );
    }
}

#[test]
fn an_unmodeled_notification_translates_to_nothing() {
    assert!(translate_notification(&notification(
        "thread/somethingNew",
        json!({ "threadId": "thr_1" })
    ))
    .is_empty());
}
