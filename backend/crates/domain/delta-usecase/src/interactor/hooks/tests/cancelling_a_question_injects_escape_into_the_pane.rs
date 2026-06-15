//! Cancelling a pending `AskUserQuestion` from the browser injects a single
//! `Escape` into the session's live TUI pane.
//!
//! A CLI hook cannot cancel the question, so Delta presses `Escape` in the pane
//! the way a human would — one key cancels the whole call. These tests assert
//! the exact key reaches the pane, and that a stale / missing pending question
//! is a graceful conflict (no key injected) so the browser falls back to the
//! terminal.

use delta_model::SessionId;

use crate::error::Error;
use crate::interactor::testing::*;

const SINGLE: &str = r#"{"questions":[{"question":"Which?","header":"Pick","options":[{"label":"A"},{"label":"B"},{"label":"C"}],"multiSelect":false}]}"#;

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
    ix.on_pre_tool_use(session, "AskUserQuestion", tool_input, "toolu_q1")
        .await
        .unwrap();
    ix.live_state_for(session)
        .await
        .pending_question
        .expect("the question is pending")
        .request_id
}

#[tokio::test]
async fn cancel_injects_a_single_escape() {
    let ix = interactor();
    ix.seed_session().await;
    let session = SessionId::from("sess-1");
    let request_id = pending_question_id(&ix, &session, SINGLE).await;

    ix.cancel_question(&session, request_id).await.unwrap();

    let keyed = ix.tmux_fake().keyed.lock().unwrap();
    assert_eq!(keyed.len(), 1, "exactly one key injection, got {keyed:?}");
    let (pane, keys) = &keyed[0];
    assert_eq!(pane, "delta-seed:0.0");
    assert_eq!(keys, &["Escape"]);
}

#[tokio::test]
async fn cancelling_a_stale_request_id_is_a_conflict_and_injects_nothing() {
    let ix = interactor();
    ix.seed_session().await;
    let session = SessionId::from("sess-1");
    let request_id = pending_question_id(&ix, &session, SINGLE).await;

    // A different id than the pending question's: already resolved or stale.
    let err = ix
        .cancel_question(&session, request_id + 999)
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::QuestionNotPending(id) if id == request_id + 999),
        "a stale id is a conflict, got {err:?}"
    );
    assert!(
        ix.tmux_fake().keyed.lock().unwrap().is_empty(),
        "nothing is injected for a stale cancel"
    );
}

#[tokio::test]
async fn cancelling_with_no_pending_question_is_a_conflict() {
    let ix = interactor();
    ix.seed_session().await;
    let session = SessionId::from("sess-1");

    let err = ix.cancel_question(&session, 1).await.unwrap_err();
    assert!(matches!(err, Error::QuestionNotPending(1)), "got {err:?}");
    assert!(ix.tmux_fake().keyed.lock().unwrap().is_empty());
}
