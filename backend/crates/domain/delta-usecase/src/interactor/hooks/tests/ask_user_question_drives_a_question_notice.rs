//! Claude Code's built-in `AskUserQuestion` tool drives a dedicated question
//! notice, not a generic Allow/Deny permission.
//!
//! - `PreToolUse` for `AskUserQuestion` emits `QuestionAsked` and mirrors the
//!   question into queryable runtime state (so a reconnecting client rebuilds
//!   its card from the sends envelope); a normal tool still emits nothing.
//! - `PermissionRequest` for `AskUserQuestion` passes straight through: no row,
//!   no waiter, no `pending_permission`, no event — so the TUI prompt appears
//!   at once and no duplicate notice is broadcast.

use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::ports::SessionEvent;

const QUESTION_INPUT: &str = r#"{"questions":[{"question":"Which?","header":"Pick","options":[{"label":"A","description":"first"},{"label":"B","description":"second"}],"multiSelect":false}]}"#;

#[tokio::test]
async fn pre_tool_use_for_ask_user_question_emits_question_asked_and_records_it() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");
    // AskUserQuestion blocks synchronously within the turn, so the question is
    // attributed to the in-flight turn's thread — here the seed prompt's main
    // thread (the latest-user-thread, falling back to main).
    let main_thread = ix.store().main_thread_id(&session).await.unwrap();

    let events = ix
        .on_pre_tool_use(
            &session,
            "AskUserQuestion",
            QUESTION_INPUT,
            "toolu_q1",
            SEED_TRANSCRIPT_PATH,
        )
        .await
        .unwrap();

    // The `QuestionAsked` event carries the PreToolUse row id, the in-flight
    // turn's thread, and the raw questions JSON, so the browser can render the
    // card on the thread it belongs to.
    let request_id = {
        let g = ix.store().inner.lock().unwrap();
        g.permissions
            .iter()
            .find(|r| r.tool_use_id.as_deref() == Some("toolu_q1"))
            .expect("the question request is recorded")
            .id
    };
    match events.as_slice() {
        [SessionEvent::QuestionAsked {
            request_id: id,
            thread_id,
            tool_input_json,
            ..
        }] => {
            assert_eq!(*id, request_id);
            assert_eq!(*thread_id, main_thread);
            assert_eq!(tool_input_json, QUESTION_INPUT);
        }
        other => panic!("expected a single QuestionAsked, got {other:?}"),
    }

    // It is also queryable as live state (re-seedable across a reconnect),
    // carrying the same thread attribution.
    let pending = ix
        .live_state_for(&session)
        .await
        .pending_question
        .expect("the question is queryable while it awaits an answer");
    assert_eq!(pending.request_id, request_id);
    assert_eq!(pending.thread_id, main_thread);
    assert_eq!(pending.tool_input_json, QUESTION_INPUT);
}

#[tokio::test]
async fn pre_tool_use_for_a_normal_tool_emits_no_question() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    let events = ix
        .on_pre_tool_use(
            &session,
            "Bash",
            r#"{"command":"ls"}"#,
            "toolu_b1",
            SEED_TRANSCRIPT_PATH,
        )
        .await
        .unwrap();

    assert!(
        events.is_empty(),
        "a normal tool records but emits no events, got {events:?}"
    );
    assert_eq!(
        ix.live_state_for(&session).await.pending_question,
        None,
        "a normal tool sets no pending question"
    );
}

#[tokio::test]
async fn permission_request_for_ask_user_question_passes_through() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");

    let before = {
        let g = ix.store().inner.lock().unwrap();
        g.permissions.len()
    };

    let wait = ix
        .on_permission_request(
            &session,
            "AskUserQuestion",
            QUESTION_INPUT,
            SEED_TRANSCRIPT_PATH,
        )
        .await
        .unwrap();

    // No event is broadcast (the question card is driven off PreToolUse), and
    // no permission row is created here (avoiding a dangling second row).
    assert!(
        wait.events.is_empty(),
        "the passthrough emits no events, got {:?}",
        wait.events
    );
    let after = {
        let g = ix.store().inner.lock().unwrap();
        g.permissions.len()
    };
    assert_eq!(
        after, before,
        "no permission row is recorded for the passthrough"
    );

    // No pending_permission is mirrored (only the PreToolUse question is).
    assert_eq!(
        ix.live_state_for(&session).await.pending_permission,
        None,
        "the passthrough sets no pending permission dialog"
    );

    // The decision receiver resolves immediately (its sender was dropped), so
    // the transport's timeout never has to wait: the TUI prompt is instant.
    assert!(
        wait.decision.await.is_err(),
        "the passthrough's decision channel is closed at once"
    );

    // Abandoning the (waiter-less) request is a safe no-op.
    ix.abandon_permission_decision(wait.request_id).await;
}
