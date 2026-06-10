use delta_model::SessionId;

use crate::interactor::testing::*;

#[tokio::test]
async fn pre_tool_use_records_request_without_notifying() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let events = ix
        .on_pre_tool_use(
            &SessionId::from("sess-1"),
            "Bash",
            r#"{"command":"ls"}"#,
            "toolu_01",
        )
        .await
        .unwrap();

    // PreToolUse fires for every tool call, so it must NOT emit the browser
    // notice; it only records the request (carrying the `tool_use_id` needed to
    // resolve it once the matching `tool_result` is ingested).
    assert!(
        events.is_empty(),
        "PreToolUse records but emits no events, got {events:?}"
    );
    let g = ix.store().inner.lock().unwrap();
    let recorded = g
        .permissions
        .iter()
        .find(|r| r.tool_use_id == "toolu_01")
        .expect("the request is recorded");
    assert_eq!(recorded.tool_name, "Bash");
    assert_eq!(recorded.tool_input_json, r#"{"command":"ls"}"#);
}
