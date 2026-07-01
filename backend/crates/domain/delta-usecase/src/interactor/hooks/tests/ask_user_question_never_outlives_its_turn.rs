//! A pending `AskUserQuestion` never outlives its turn.
//!
//! The question card mirrors the permission notice's lifecycle: it is swept
//! when the turn returns to idle (the `Stop` hook here), since the question
//! blocked that turn and the turn ending makes it moot.

use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::ports::StopHook;

const QUESTION_INPUT: &str =
    r#"{"questions":[{"question":"Which?","header":"Pick","options":[],"multiSelect":false}]}"#;

#[tokio::test]
async fn the_turn_ending_clears_the_queryable_question() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    ix.on_pre_tool_use(
        &session,
        "AskUserQuestion",
        QUESTION_INPUT,
        "toolu_q1",
        SEED_TRANSCRIPT_PATH,
    )
    .await
    .unwrap();
    assert!(
        ix.live_state_for(&session).await.pending_question.is_some(),
        "the question is pending after PreToolUse"
    );

    // The turn ends (Stop hook): the question cannot outlive its turn.
    ix.on_stop(StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();

    assert_eq!(
        ix.live_state_for(&session).await.pending_question,
        None,
        "a question never outlives its turn"
    );
}
