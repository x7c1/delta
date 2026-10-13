//! A question the agent asks the user.

use super::*;

#[test]
fn question_asked_serializes_as_a_tagged_event_carrying_raw_tool_input() {
    assert_eq!(
        json(&WireSessionEvent::from(SessionEvent::QuestionAsked {
            session_id: SessionId::from("sess-1"),
            request_id: 9,
            thread_id: ThreadId(3),
            tool_input_json: "{\"questions\":[{\"header\":\"Pick\"}]}".to_owned(),
        })),
        serde_json::json!({
            "kind": "question_asked",
            "session_id": "sess-1",
            "request_id": 9,
            "thread_id": 3,
            "tool_input": "{\"questions\":[{\"header\":\"Pick\"}]}",
        }),
    );
}
