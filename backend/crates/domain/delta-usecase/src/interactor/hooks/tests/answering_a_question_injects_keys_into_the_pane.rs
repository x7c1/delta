//! Answering a pending `AskUserQuestion` from the browser injects the pinned
//! selection keystrokes into the session's live TUI pane.
//!
//! A CLI hook cannot return the user's pick, so Delta drives the TUI widget by
//! injecting keys. These tests assert the exact key list reaches the pane for a
//! single-select pick, and that a stale / unparseable / malformed answer is a
//! graceful error (no keys injected) so the browser falls back to the terminal.

use delta_model::SessionId;

use crate::error::Error;
use crate::interactor::testing::*;

const SINGLE: &str = r#"{"questions":[{"question":"Which?","header":"Pick","options":[{"label":"A"},{"label":"B"},{"label":"C"}],"multiSelect":false}]}"#;
const MULTI_SELECT: &str = r#"{"questions":[{"question":"Which?","header":"Pick","options":[{"label":"A"},{"label":"B"},{"label":"C"}],"multiSelect":true}]}"#;
/// Two questions: Q1 single-select (2 options), Q2 multi-select (3 options).
const MULTI_QUESTION_MULTI_SELECT: &str = r#"{"questions":[{"question":"Q1?","header":"One","options":[{"label":"A"},{"label":"B"}],"multiSelect":false},{"question":"Q2?","header":"Two","options":[{"label":"X"},{"label":"Y"},{"label":"Z"}],"multiSelect":true}]}"#;

/// Record a pending question on the seeded (open) session and return its
/// request id.
async fn pending_question_id(
    ix: &crate::interactor::Interactor<
        FakeTmux,
        FakeTranscript,
        FakeStore,
        FakeWorkspace,
        FakeGitWorktree,
    >,
    session: &SessionId,
    tool_input: &str,
) -> i64 {
    ix.on_pre_tool_use(session, "AskUserQuestion", tool_input, "toolu_q1", SEED_TRANSCRIPT_PATH)
        .await
        .unwrap();
    ix.live_state_for(session)
        .await
        .pending_question
        .expect("the question is pending")
        .request_id
}

#[tokio::test]
async fn single_select_answer_injects_down_times_index_then_enter() {
    let ix = interactor();
    ix.seed_session().await;
    let session = SessionId::from("sess-1");
    let request_id = pending_question_id(&ix, &session, SINGLE).await;

    // Pick option 2 (0-based): Down twice to reach it, then Enter submits.
    ix.answer_question(&session, request_id, vec![vec![2]])
        .await
        .unwrap();

    let keyed = ix.tmux_fake().keyed.lock().unwrap();
    assert_eq!(keyed.len(), 1, "exactly one key injection, got {keyed:?}");
    let (pane, keys) = &keyed[0];
    assert_eq!(pane, "delta-seed:0.0");
    assert_eq!(keys, &["Down", "Down", "Enter"]);
}

#[tokio::test]
async fn multi_select_answer_toggles_each_then_submits() {
    let ix = interactor();
    ix.seed_session().await;
    let session = SessionId::from("sess-1");
    let request_id = pending_question_id(&ix, &session, MULTI_SELECT).await;

    // Toggle options 0 and 2, then Right to the Submit tab and Enter.
    ix.answer_question(&session, request_id, vec![vec![0, 2]])
        .await
        .unwrap();

    let keyed = ix.tmux_fake().keyed.lock().unwrap();
    assert_eq!(
        keyed[0].1,
        &["Space", "Down", "Down", "Space", "Right", "Enter"],
    );
}

#[tokio::test]
async fn multi_question_with_a_multi_select_injects_the_full_review_sequence() {
    let ix = interactor();
    ix.seed_session().await;
    let session = SessionId::from("sess-1");
    let request_id = pending_question_id(&ix, &session, MULTI_QUESTION_MULTI_SELECT).await;

    // Q1 single-select pick option 0 (lone Enter records + advances); Q2
    // multi-select toggle options 0 and 2 (Space, Down, Down, Space), Right
    // advances to the review screen; a final Enter submits the review.
    ix.answer_question(&session, request_id, vec![vec![0], vec![0, 2]])
        .await
        .unwrap();

    let keyed = ix.tmux_fake().keyed.lock().unwrap();
    assert_eq!(
        keyed[0].1,
        &["Enter", "Space", "Down", "Down", "Space", "Right", "Enter"],
    );
}

#[tokio::test]
async fn answering_a_stale_request_id_is_a_conflict_and_injects_nothing() {
    let ix = interactor();
    ix.seed_session().await;
    let session = SessionId::from("sess-1");
    let request_id = pending_question_id(&ix, &session, SINGLE).await;

    // A different id than the pending question's: already answered or stale.
    let err = ix
        .answer_question(&session, request_id + 999, vec![vec![0]])
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::QuestionNotPending(id) if id == request_id + 999),
        "a stale id is a conflict, got {err:?}"
    );
    assert!(
        ix.tmux_fake().keyed.lock().unwrap().is_empty(),
        "nothing is injected for a stale answer"
    );
}

#[tokio::test]
async fn answering_with_no_pending_question_is_a_conflict() {
    let ix = interactor();
    ix.seed_session().await;
    let session = SessionId::from("sess-1");

    let err = ix
        .answer_question(&session, 1, vec![vec![0]])
        .await
        .unwrap_err();
    assert!(matches!(err, Error::QuestionNotPending(1)), "got {err:?}");
    assert!(ix.tmux_fake().keyed.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_malformed_selection_is_a_bad_request_and_injects_nothing() {
    let ix = interactor();
    ix.seed_session().await;
    let session = SessionId::from("sess-1");
    let request_id = pending_question_id(&ix, &session, SINGLE).await;

    // Option index out of range for a 3-option question.
    let err = ix
        .answer_question(&session, request_id, vec![vec![9]])
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::InvalidQuestionAnswer(_)),
        "an out-of-range option is a bad answer, got {err:?}"
    );
    assert!(
        ix.tmux_fake().keyed.lock().unwrap().is_empty(),
        "nothing is injected for a malformed answer"
    );
}
