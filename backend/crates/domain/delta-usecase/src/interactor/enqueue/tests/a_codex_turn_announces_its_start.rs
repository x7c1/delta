use std::time::Duration;

use delta_model::{AgentProvider, MessageUuid, SessionId, ThreadId};

use crate::agent::{AgentEvent, TurnStatus};
use crate::interactor::testing::*;
use crate::ports::{AsyncEventReceiver, SessionEvent};
use crate::SendTarget;

/// Collect the turn-boundary events (`TurnStarted` / `TurnCompleted`) off the
/// async seam until a `TurnCompleted` arrives, or panic after a short deadline.
/// Everything else on the seam (transcript updates, `SessionRegistered`) is
/// skipped.
async fn turn_events_until_completed(events: &mut AsyncEventReceiver) -> Vec<SessionEvent> {
    let mut seen = Vec::new();
    loop {
        let event = tokio::time::timeout(Duration::from_secs(5), events.recv())
            .await
            .expect("timed out waiting for the turn to complete")
            .expect("the event seam stays open");
        match event {
            SessionEvent::TurnStarted { .. } => seen.push(event),
            SessionEvent::TurnCompleted { .. } => {
                seen.push(event);
                return seen;
            }
            _ => {}
        }
    }
}

/// Assert `turn` is exactly one `TurnStarted` naming `send_id` on `thread_id`
/// (matched with the provider's turn id the row was marked with), followed by
/// that turn's `TurnCompleted` on the same thread.
fn assert_one_start_then_end(
    turn: &[SessionEvent],
    session_id: &SessionId,
    send_id: i64,
    thread_id: ThreadId,
    what: &str,
) {
    assert_eq!(
        turn,
        [
            SessionEvent::TurnStarted {
                session_id: session_id.clone(),
                send_id: Some(send_id),
                thread_id,
                matched_uuid: Some(MessageUuid::from("turn_start")),
            },
            SessionEvent::TurnCompleted {
                session_id: session_id.clone(),
                thread_id: Some(thread_id),
                stop_reason: None,
            },
        ],
        "{what}: one start naming the send and its thread, then the turn's end"
    );
}

/// An adapter-backed (Codex) session has no prompt hook, so its turns are
/// announced from the `turn/start` acknowledgement: every turn — the opening
/// one and every later send — emits exactly one `SessionEvent::TurnStarted`
/// naming its send and the send's thread, before the turn's `TurnCompleted`.
/// This is what lights the running indicator on a session that is not open.
///
/// The provider's own `AgentEvent::TurnStarted` arriving mid-turn must not be
/// announced a second time.
#[tokio::test]
async fn a_codex_turn_announces_its_start() {
    let factory = FakeAgentFactory::new("thr_announce", Some("turn_start"));
    let stream = factory.event_sender();
    let (ix, mut events) = interactor_with_codex_factory_and_event_sink(factory);

    // The opening turn.
    let (first, _) = ix
        .enqueue_send(
            SendTarget::NewSession {
                pull_request_number: None,
                provider: AgentProvider::Codex,
                workdir: TEST_WORKDIR.to_owned(),
                launch_option_ids: Vec::new(),
                worktree: None,
            },
            "first message",
            None,
        )
        .await
        .unwrap();
    ix.await_launch().await;
    let session_id = first.session_id.clone();
    let thread_id = first.thread_id;

    stream
        .send(AgentEvent::TurnStarted {
            provider_turn_id: Some("turn_start".to_owned()),
        })
        .expect("the pump's stream is live");
    stream
        .send(AgentEvent::TurnCompleted {
            status: TurnStatus::Completed,
        })
        .expect("the pump's stream is live");
    let opening = turn_events_until_completed(&mut events).await;
    assert_one_start_then_end(
        &opening,
        &session_id,
        first.id,
        thread_id,
        "the opening turn",
    );

    // A later send into the now-idle session.
    let (second, sync_events) = ix
        .enqueue_send(
            SendTarget::Thread {
                thread_id,
                branch_from: None,
            },
            "second message",
            None,
        )
        .await
        .unwrap();
    assert!(
        sync_events.is_empty(),
        "the announcement leaves on the async seam, not the send's reply"
    );
    stream
        .send(AgentEvent::TurnStarted {
            provider_turn_id: Some("turn_start".to_owned()),
        })
        .expect("the pump's stream is live");
    stream
        .send(AgentEvent::TurnCompleted {
            status: TurnStatus::Completed,
        })
        .expect("the pump's stream is live");
    let later = turn_events_until_completed(&mut events).await;
    assert_one_start_then_end(&later, &session_id, second.id, thread_id, "a later send");
}
