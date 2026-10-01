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
//!
//! Some local commands do not print and exit but leave a dialog up (`/cost`
//! the usage panel, `/model` the model picker), which would swallow the next
//! send's keystrokes — the picker would even take its Enter as a selection. So
//! the settle presses a single `Escape` into the pane before the queue behind
//! the command is flushed. A slash command that echoes (a skill or custom
//! command, really a prompt) never reaches the deadline and gets no `Escape`.

use std::time::{Duration, Instant};

use delta_model::{SendStatus, SessionId};

use crate::interactor::session_actor::runtime::SLASH_COMMAND_ECHO_DEADLINE;
use crate::interactor::testing::*;
use crate::ports::{SessionEvent, StopHook};
use crate::turn::TurnState;

/// The pane the seeded session's keystrokes go into.
const PANE: &str = "delta-seed:0.0";

/// An instant just past the slash-command deadline measured from `from`: well
/// short of the plain-prompt deadline.
fn past_slash_command_deadline(from: Instant) -> Instant {
    from + SLASH_COMMAND_ECHO_DEADLINE + Duration::from_secs(1)
}

/// `/cost` leaves no trace at all; its short deadline settles it as delivered
/// with no re-type, a single `Escape` dismisses the dialog it may have left up,
/// and only then does the send queued behind it dispatch, in the same tick.
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
            PaneInput::Keys {
                pane: PANE.to_owned(),
                keys: vec!["Escape".to_owned()],
            },
            PaneInput::Line {
                pane: PANE.to_owned(),
                text: "waiting behind the command".to_owned(),
            },
        ],
        "the command is typed exactly once, and exactly one Escape reaches the \
         pane before the next send is typed",
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

/// A slash command that is really a prompt (a skill, a custom command) echoes
/// `UserPromptSubmit` and runs an ordinary turn. It opened no dialog, so
/// nothing presses `Escape` — neither when its turn ends and the queue behind
/// it flushes, nor from a later sweep past the slash-command deadline.
#[tokio::test]
async fn an_echoed_slash_command_presses_no_escape() {
    let ix = interactor();
    ix.seed_session().await;
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    let (skill, _) = ix.enqueue_send(to(main), "/my-skill", None).await.unwrap();
    assert_eq!(skill.status, SendStatus::Dispatched);
    let (behind, _) = ix
        .enqueue_send(to(main), "waiting behind the skill", None)
        .await
        .unwrap();
    assert_eq!(behind.status, SendStatus::Queued);

    // The skill echoes well inside the deadline and runs as a turn.
    ix.transcript_fake().push(user_line("u-1", "/my-skill"));
    ix.on_user_prompt_submit(submit("/my-skill")).await.unwrap();
    assert_eq!(
        ix.live_state_for(&session).await.turn,
        TurnState::InFlight {
            send_id: Some(skill.id)
        },
    );

    // A sweep past the slash-command deadline finds no wait to settle.
    let dispatched = ix
        .sweep_echo_deadlines(past_slash_command_deadline(Instant::now()), TICK_BOUND)
        .await
        .unwrap();
    assert!(dispatched.is_empty(), "got {dispatched:?}");

    // The turn ends and the queue behind the skill flushes.
    ix.on_stop(StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();
    assert_eq!(
        ix.store().send(behind.id).await.unwrap().unwrap().status,
        SendStatus::Dispatched,
    );

    assert_eq!(
        ix.tmux_fake().pane_input.lock().unwrap().clone(),
        vec![
            PaneInput::Line {
                pane: PANE.to_owned(),
                text: "/my-skill".to_owned(),
            },
            PaneInput::Line {
                pane: PANE.to_owned(),
                text: "waiting behind the skill".to_owned(),
            },
        ],
        "an echoed slash command gets no Escape before the next send",
    );
}

/// The `Escape` is best-effort: a pane that refuses the key injection still
/// sees the command settled and the session released, so a failed dismissal
/// can never wedge the queue behind a command that ran.
#[tokio::test]
async fn a_failed_escape_still_settles_the_command() {
    let ix = interactor();
    ix.seed_session().await;
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    let (command, _) = ix.enqueue_send(to(main), "/model", None).await.unwrap();
    let (behind, _) = ix
        .enqueue_send(to(main), "waiting behind the picker", None)
        .await
        .unwrap();
    ix.tmux_fake().fail_key_injection();

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
        "the queue behind the command still moves on",
    );
    assert_eq!(
        ix.store().send(command.id).await.unwrap().unwrap().status,
        SendStatus::Matched,
        "the command settles even though its dialog could not be dismissed",
    );
    assert_eq!(
        ix.live_state_for(&session).await.turn,
        TurnState::AwaitingEcho {
            send_id: behind.id,
            slash_command: false,
        },
    );
}
