use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::ports::SessionEvent;

#[tokio::test]
async fn pre_tool_use_records_request_and_notifies() {
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
    assert!(events.iter().any(|e| matches!(
        e,
        SessionEvent::PermissionRequested { tool_name, .. } if tool_name == "Bash"
    )));
}
