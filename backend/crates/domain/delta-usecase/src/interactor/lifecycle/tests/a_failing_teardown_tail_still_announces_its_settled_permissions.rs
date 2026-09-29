//! A teardown whose last two steps fail must still hand back the settle's
//! events.
//!
//! The bound teardown settles the pending permission requests — denying their
//! rows and building the `PermissionResolved` events that clear the browser's
//! dialogs — and only then closes the turn and sweeps the background subagents.
//! Both of those write rows. If either write fails, the denied rows can never
//! produce their events again, so the teardown must log the failing step and
//! still return the events rather than drop them.

use std::time::Instant;

use delta_model::{PermissionStatus, SessionId};

use crate::interactor::testing::*;
use crate::ports::{SessionEvent, StopHook};
use crate::turn::TurnState;

/// The request ids the returned batch settled, in order.
fn resolved_ids(events: &[SessionEvent]) -> Vec<i64> {
    events
        .iter()
        .filter_map(|event| match event {
            SessionEvent::PermissionResolved { request_id, .. } => Some(*request_id),
            _ => None,
        })
        .collect()
}

/// Raise a permission dialog on `session`, returning its request id.
async fn raise_dialog(ix: &TestInteractor, session: &SessionId, transcript: &str) -> i64 {
    ix.on_permission_request(
        session,
        "Bash",
        r#"{"command":"rm -rf /tmp/x"}"#,
        transcript,
    )
    .await
    .unwrap()
    .request_id
}

/// `sess-1` open and bound, with a send dispatched but not yet echoed — so the
/// teardown's `TurnInput::Close` has a row to cancel — and a dialog up.
async fn session_with_an_unechoed_send_and_a_dialog() -> (TestInteractor, SessionId, i64) {
    let ix = interactor();
    let session = SessionId::from("sess-1");
    ix.seed_session().await;
    let main = ix.store().main_thread_id(&session).await.unwrap();
    let (send, _) = ix.enqueue_send(to(main), "go", None).await.unwrap();
    assert_eq!(
        ix.live_state_for(&session).await.turn,
        TurnState::AwaitingEcho { send_id: send.id },
        "the close has an outstanding send to cancel"
    );
    let request_id = raise_dialog(&ix, &session, SEED_TRANSCRIPT_PATH).await;
    (ix, session, request_id)
}

fn assert_denied(ix: &TestInteractor, request_id: i64) {
    let rows = ix.store().inner.lock().unwrap().permissions.clone();
    let row = rows
        .iter()
        .find(|row| row.id == request_id)
        .expect("the request was recorded");
    assert_eq!(row.status, PermissionStatus::Denied, "{row:?}");
}

#[tokio::test]
async fn a_close_whose_turn_close_fails_still_announces_the_settled_dialog() {
    let (logs, _guard) = capture_warnings();
    let (ix, session, request_id) = session_with_an_unechoed_send_and_a_dialog().await;
    ix.store().inner.lock().unwrap().fail_cancel_send = true;

    let events = ix
        .close_session(&session)
        .await
        .expect("a failure past the point of no return does not fail the close");

    assert_eq!(
        resolved_ids(&events),
        vec![request_id],
        "the dialog the settle denied is still announced: {events:?}"
    );
    assert_denied(&ix, request_id);
    assert!(
        ix.bound_pane(&session).await.is_none(),
        "the session is closed"
    );
    assert_eq!(
        ix.live_state_for(&session).await.turn,
        TurnState::Idle,
        "the turn machine is closed even though the row write failed"
    );
    let logged = logs.text();
    assert!(
        logged.contains("WARN")
            && logged.contains("session_id=sess-1")
            && logged.contains("step=\"turn_close\""),
        "the failing step is logged with the session: {logged}"
    );
}

#[tokio::test]
async fn a_close_whose_subagent_sweep_fails_still_announces_the_settled_dialog() {
    let (logs, _guard) = capture_warnings();
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");
    ix.bind_open_session("delta-seed", &session).await;

    // A background subagent that outlives its launching turn, so the teardown's
    // sweep has a launch row to clear.
    ix.transcript_fake()
        .push(background_tool_use_line("a-launch", "toolu_bg"));
    ix.on_pre_tool_use(
        &session,
        "Agent",
        r#"{"subagent_type":"general-purpose","description":"Long crawl","run_in_background":true}"#,
        "toolu_bg",
        SEED_TRANSCRIPT_PATH,
    )
    .await
    .unwrap();
    ix.on_stop(StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();
    ix.on_user_prompt_submit(submit("next")).await.unwrap();
    let request_id = raise_dialog(&ix, &session, SEED_TRANSCRIPT_PATH).await;
    ix.store().inner.lock().unwrap().fail_clear_subagent_launch = true;

    let events = ix
        .close_session(&session)
        .await
        .expect("a failure past the point of no return does not fail the close");

    // The `Agent` call's own pre-tool-use row is settled too (its tool_result
    // never arrived), so the dialog is one of the announced resolutions rather
    // than the only one.
    assert!(
        resolved_ids(&events).contains(&request_id),
        "the dialog the settle denied is still announced: {events:?}"
    );
    assert_denied(&ix, request_id);
    assert!(
        ix.bound_pane(&session).await.is_none(),
        "the session is closed"
    );
    let logged = logs.text();
    assert!(
        logged.contains("WARN")
            && logged.contains("session_id=sess-1")
            && logged.contains("step=\"subagent_sweep\""),
        "the failing step is logged with the session: {logged}"
    );
}

#[tokio::test]
async fn a_vanished_pane_whose_turn_close_fails_still_announces_the_settled_dialog() {
    // The pane-gone close runs the same routine, so the same failure keeps the
    // same events — and the `SessionClosed` still follows them.
    const TRANSCRIPT: &str = "/work/delta-1/t.jsonl";
    let ix = interactor();
    ix.new_session().await.unwrap();
    let session: SessionId = ix.pending_session_ids().await.remove(0);
    ix.on_user_prompt_submit(submit_in(
        session.as_str(),
        TRANSCRIPT,
        "/work/delta-1",
        "hi",
    ))
    .await
    .unwrap();
    ix.on_stop(StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();
    let main = ix.store().main_thread_id(&session).await.unwrap();
    let (send, _) = ix.enqueue_send(to(main), "go", None).await.unwrap();
    assert_eq!(
        ix.live_state_for(&session).await.turn,
        TurnState::AwaitingEcho { send_id: send.id },
        "the close has an outstanding send to cancel"
    );
    let request_id = raise_dialog(&ix, &session, TRANSCRIPT).await;
    ix.store().inner.lock().unwrap().fail_cancel_send = true;

    ix.tmux_fake().vanish_session("delta-1");
    let events = ix
        .reap_stale_spawns(Instant::now(), TICK_BOUND)
        .await
        .unwrap();

    assert_eq!(
        resolved_ids(&events),
        vec![request_id],
        "the dialog the settle denied is still announced: {events:?}"
    );
    assert!(
        matches!(
            events.last(),
            Some(SessionEvent::SessionClosed { session_id }) if *session_id == session
        ),
        "the session is still announced closed, after the settle: {events:?}"
    );
    assert_denied(&ix, request_id);
    assert!(
        ix.bound_pane(&session).await.is_none(),
        "the session is closed"
    );
}
