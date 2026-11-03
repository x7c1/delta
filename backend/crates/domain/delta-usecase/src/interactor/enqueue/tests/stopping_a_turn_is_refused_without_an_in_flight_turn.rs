//! Stopping a turn is refused — before any keystroke reaches the pane — when
//! Delta holds no in-flight turn of a live pane: an idle session, one still
//! awaiting a dispatched send's echo, a closed session, and a Codex session.
//! None of them gets a keystroke, an event, or a dispatch.

use delta_model::{AgentProvider, SendStatus, SessionId};

use crate::error::Error;
use crate::interactor::testing::*;
use crate::turn::TurnState;
use crate::turn_stop_refusal::TurnStopRefusal;
use crate::SendTarget;

/// Assert the stop was refused for the reason matching `$reason`, matching
/// the variant rather than comparing values.
macro_rules! assert_refused {
    ($err:expr, $reason:pat) => {{
        let err = $err;
        assert!(
            matches!(
                err,
                Error::TurnNotStoppable {
                    reason: $reason,
                    ..
                }
            ),
            "expected a {} refusal, got {err:?}",
            stringify!($reason),
        );
    }};
}

/// An idle session has nothing to stop.
#[tokio::test]
async fn stopping_an_idle_session_is_refused() {
    let ix = interactor();
    let session = SessionId::from("sess-1");
    ix.seed_session().await;

    let err = ix.stop_turn(&session).await.unwrap_err();

    assert_refused!(err, TurnStopRefusal::Idle);
    assert!(ix.tmux_fake().sent.lock().unwrap().is_empty());
    assert!(ix.tmux_fake().keyed.lock().unwrap().is_empty());
    assert_eq!(ix.live_state_for(&session).await.turn, TurnState::Idle);
}

/// A dispatched send still awaiting its echo has its own exits (cancel, the
/// echo deadline); a stop there would race the echo and is refused,
/// leaving the queued send behind it queued.
#[tokio::test]
async fn stopping_while_an_echo_is_awaited_is_refused() {
    let ix = interactor();
    let session = SessionId::from("sess-1");
    ix.seed_session().await;
    let main = ix.store().main_thread_id(&session).await.unwrap();

    let (first, _) = ix.enqueue_send(to(main), "first", None).await.unwrap();
    let (second, _) = ix.enqueue_send(to(main), "second", None).await.unwrap();
    assert_eq!(second.status, SendStatus::Queued);

    let err = ix.stop_turn(&session).await.unwrap_err();

    assert_refused!(err, TurnStopRefusal::AwaitingEcho);
    assert_eq!(ix.tmux_fake().sent.lock().unwrap().len(), 1);
    assert!(ix.tmux_fake().keyed.lock().unwrap().is_empty());
    assert_eq!(
        ix.store().send(second.id).await.unwrap().unwrap().status,
        SendStatus::Queued,
    );
    assert_eq!(
        ix.live_state_for(&session).await.turn,
        TurnState::AwaitingEcho {
            send_id: first.id,
            slash_command: false,
        }
    );
}

/// A closed session has no pane and nothing running in it.
#[tokio::test]
async fn stopping_a_closed_session_is_refused() {
    let ix = interactor();
    let session = SessionId::from("sess-1");
    ix.seed_session().await;
    ix.on_user_prompt_submit(submit("typed in the pane"))
        .await
        .unwrap();
    ix.close_session(&session).await.unwrap();
    let sent_before = ix.tmux_fake().sent.lock().unwrap().len();

    let err = ix.stop_turn(&session).await.unwrap_err();

    assert_refused!(err, TurnStopRefusal::NotOpen);
    assert_eq!(ix.tmux_fake().sent.lock().unwrap().len(), sent_before);
    assert!(ix.tmux_fake().keyed.lock().unwrap().is_empty());
}

/// An unknown session id is not a refusal but a missing session.
#[tokio::test]
async fn stopping_an_unknown_session_is_session_not_found() {
    let ix = interactor();

    let err = ix.stop_turn(&SessionId::from("ghost")).await.unwrap_err();

    assert!(
        matches!(&err, Error::SessionNotFound(id) if id == "ghost"),
        "got {err:?}"
    );
}

/// A Codex session is stopped through its provider's own interrupt, so the
/// stop is refused and its running turn is untouched.
#[tokio::test]
async fn stopping_a_codex_session_is_refused() {
    let ix = interactor_with_codex_factory(FakeAgentFactory::new("thr_stop", Some("turn_1")));
    let (send, _) = ix
        .enqueue_send(
            SendTarget::NewSession {
                pull_request_number: None,
                provider: AgentProvider::Codex,
                workdir: TEST_WORKDIR.to_owned(),
                launch_option_ids: Vec::new(),
                worktree: None,
            },
            "long task",
            None,
        )
        .await
        .unwrap();
    ix.await_launch().await;
    let session = send.session_id.clone();
    assert_eq!(
        ix.live_state_for(&session).await.turn,
        TurnState::InFlight { send_id: None },
    );

    let err = ix.stop_turn(&session).await.unwrap_err();

    assert_refused!(err, TurnStopRefusal::NoTerminal);
    assert_eq!(
        ix.live_state_for(&session).await.turn,
        TurnState::InFlight { send_id: None },
        "the Codex turn is left to its provider"
    );
}
