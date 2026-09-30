//! A local slash command that Claude Code records nothing for still ends its
//! turn.
//!
//! From Claude Code 2.1.286 a local command such as `/cost` fires no
//! `UserPromptSubmit`, no `Stop`, and writes no transcript line — the
//! caveat / command-name / stdout group older versions wrote (and the fold
//! still resolves) never appears. The only thing left to observe is the
//! silence, so a slash-command send's echo deadline is short and settles the
//! send as delivered: the session frees, the command is never re-typed (that
//! would run it twice), and the queue behind it moves on.

use std::time::{Duration, Instant};

use delta_model::{SendStatus, SessionId};

use crate::interactor::session_actor::runtime::SLASH_COMMAND_ECHO_DEADLINE;
use crate::interactor::testing::*;
use crate::ports::SessionEvent;
use crate::turn::TurnState;

/// The pane the seeded session's keystrokes go into.
const PANE: &str = "delta-seed:0.0";

/// An instant just past the slash-command deadline measured from `from`: well
/// short of the plain-prompt deadline.
fn past_slash_command_deadline(from: Instant) -> Instant {
    from + SLASH_COMMAND_ECHO_DEADLINE + Duration::from_secs(1)
}

/// `/cost` leaves no trace at all; its short deadline settles it as delivered,
/// with no `Escape` and no re-type, and the send queued behind it dispatches in
/// the same tick.
#[tokio::test]
async fn a_local_command_send_frees_the_session_at_its_short_deadline() {
    let (ix, mut events) = interactor_with_event_sink();
    ix.seed_session().await;
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    let (command, _) = ix.enqueue_send(to(main), "/cost", None).await.unwrap();
    assert_eq!(command.status, SendStatus::Dispatched);
    assert_eq!(
        ix.live_state_for(&session).await.turn,
        TurnState::AwaitingEcho {
            send_id: command.id,
            slash_command: true,
        },
        "the wait remembers the send is a slash command",
    );

    // Composed while the command is outstanding: it waits its turn.
    let (behind, _) = ix
        .enqueue_send(to(main), "waiting behind the command", None)
        .await
        .unwrap();
    assert_eq!(behind.status, SendStatus::Queued);

    // Nothing is ever heard about the command: no hook, no transcript line.
    // Before its deadline the wait is left alone.
    let dispatched = ix
        .sweep_echo_deadlines(Instant::now(), TICK_BOUND)
        .await
        .unwrap();
    assert!(dispatched.is_empty(), "no deadline has passed yet");

    // Past the slash-command deadline: the command is taken to have run.
    let dispatched = ix
        .sweep_echo_deadlines(past_slash_command_deadline(Instant::now()), TICK_BOUND)
        .await
        .unwrap();
    assert_eq!(
        dispatched,
        vec![SessionEvent::SendDispatched {
            session_id: session.clone(),
            send_id: behind.id,
        }],
        "the send queued behind the command dispatches in the same tick",
    );

    let settled = ix.store().send(command.id).await.unwrap().unwrap();
    assert_eq!(
        settled.status,
        SendStatus::Matched,
        "the command settles as delivered, not requeued, parked or cancelled",
    );
    assert!(settled.held_at.is_none(), "the command is not parked");
    assert_eq!(
        ix.tmux_fake().pane_input.lock().unwrap().clone(),
        vec![
            PaneInput::Line {
                pane: PANE.to_owned(),
                text: "/cost".to_owned(),
            },
            PaneInput::Line {
                pane: PANE.to_owned(),
                text: "waiting behind the command".to_owned(),
            },
        ],
        "the command is typed exactly once, with no Escape before the next send",
    );
    assert_eq!(
        ix.live_state_for(&session).await.turn,
        TurnState::AwaitingEcho {
            send_id: behind.id,
            slash_command: false,
        },
        "the session moved on to the next send's own wait",
    );
    let command_id = command.id;
    let spent = ix
        .with_runtime(&session, move |state| state.requeues_spent(command_id))
        .await;
    assert_eq!(spent, 0, "settling a command spends no requeue budget");

    // The browser is told the command's turn ended, the same way an older
    // transcript's command line tells it, and nothing is announced as parked.
    let mut drained = Vec::new();
    while let Ok(event) = events.try_recv() {
        drained.push(event);
    }
    assert!(
        drained.contains(&SessionEvent::TurnInterrupted {
            session_id: session.clone(),
            thread_id: Some(main),
        }),
        "the settle clears the browser's in-progress state; got {drained:?}"
    );
    assert!(
        !drained
            .iter()
            .any(|event| matches!(event, SessionEvent::SendParked { .. })),
        "a command that ran is not parked; got {drained:?}"
    );
}

/// The short deadline is for slash commands only: a plain prompt that has been
/// silent just as long is still a slow echo, not a lost one, and is left alone
/// until the ordinary deadline.
#[tokio::test]
async fn a_plain_send_is_not_released_at_the_slash_command_deadline() {
    let ix = interactor();
    ix.seed_session().await;
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    let (send, _) = ix
        .enqueue_send(to(main), "a plain prompt", None)
        .await
        .unwrap();

    let dispatched = ix
        .sweep_echo_deadlines(past_slash_command_deadline(Instant::now()), TICK_BOUND)
        .await
        .unwrap();
    assert!(dispatched.is_empty(), "got {dispatched:?}");
    assert_eq!(
        ix.store().send(send.id).await.unwrap().unwrap().status,
        SendStatus::Dispatched,
    );
    assert_eq!(
        ix.live_state_for(&session).await.turn,
        TurnState::AwaitingEcho {
            send_id: send.id,
            slash_command: false,
        },
    );
}
