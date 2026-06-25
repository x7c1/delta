use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::ports::SessionEvent;

const QUESTION_INPUT: &str =
    r#"{"questions":[{"question":"Which?","header":"Pick","options":[],"multiSelect":false}]}"#;

/// Answering an `AskUserQuestion` in the TUI flushes a `tool_result` for its
/// tool_use_id; ingesting it resolves the question's request row (emitting
/// `PermissionResolved` for it) and clears the queryable pending question, so
/// the browser's question card clears just like a permission notice.
#[tokio::test]
async fn ingesting_tool_result_resolves_the_ask_user_question() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");
    ix.bind_open_session("delta-1", &session).await;

    ix.on_pre_tool_use(&session, "AskUserQuestion", QUESTION_INPUT, "toolu_q1", SEED_TRANSCRIPT_PATH)
        .await
        .unwrap();
    let request_id = {
        let g = ix.store().inner.lock().unwrap();
        g.permissions
            .iter()
            .find(|r| r.tool_use_id.as_deref() == Some("toolu_q1"))
            .expect("the question request is recorded")
            .id
    };
    assert!(
        ix.live_state_for(&session)
            .await
            .pending_question
            .is_some(),
        "the question is pending after PreToolUse"
    );

    // The user answered in the TUI: the correlated tool_result lands.
    ix.transcript_fake()
        .push(tool_result_line("r-q1", "toolu_q1"));
    let (_groups, events) = ix.poll_transcript().await.unwrap();

    assert!(
        events.iter().any(|e| matches!(
            e,
            SessionEvent::PermissionResolved { request_id: id, .. } if *id == request_id
        )),
        "the tool_result resolves the question's request row"
    );
    assert_eq!(
        ix.live_state_for(&session).await.pending_question,
        None,
        "the pending question is cleared once it resolves"
    );
}
