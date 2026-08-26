//! The compact recovery re-types only what nobody has heard about yet.
//!
//! A send whose echo Claude Code rewrote stays `dispatched` for the whole turn
//! — no transcript line carries its text, so nothing claims the row until the
//! turn ends and it is settled as delivered. The row alone therefore cannot say
//! whether a send is stuck; the turn machine can. Re-typing a send that was
//! already consumed would deliver the same message twice, which is exactly the
//! duplicate positional consumption exists to remove.

use delta_model::{SendStatus, SessionId};

use crate::interactor::testing::*;
use crate::turn::TurnState;

#[tokio::test]
async fn compact_does_not_retype_a_consumed_send() {
    let ix = interactor();
    let session = SessionId::from("sess-1");
    ix.seed_session().await;
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // Dispatched and awaiting its echo: one set of keystrokes so far.
    let (send, _) = ix
        .enqueue_send(to(main), "/plan the migration", None)
        .await
        .unwrap();
    assert_eq!(send.status, SendStatus::Dispatched);
    assert_eq!(ix.tmux_fake().sent.lock().unwrap().len(), 1);

    // Claude Code submits the prompt under a rewritten text. The send is
    // consumed by position, so the turn is now the send's — but no transcript
    // line matches it, so its row stays `dispatched` until the turn ends.
    ix.transcript_fake()
        .push(user_line("u-1", "<command-name>/plan</command-name>"));
    ix.on_user_prompt_submit(submit("<command-name>/plan</command-name>"))
        .await
        .unwrap();
    assert_eq!(
        ix.live_state_for(&session).await.turn,
        TurnState::InFlight {
            send_id: Some(send.id)
        },
    );
    assert_eq!(
        ix.store().send(send.id).await.unwrap().unwrap().status,
        SendStatus::Dispatched,
    );

    // An auto-`/compact` fires mid-turn. The row still reads `dispatched`, but
    // the turn machine is not awaiting anything: the message was delivered, so
    // the recovery must leave it alone.
    ix.on_session_start(session_start(session.as_str(), "compact"))
        .await
        .unwrap();

    let sent = ix.tmux_fake().sent.lock().unwrap().clone();
    assert_eq!(
        sent.len(),
        1,
        "a consumed send must not be re-typed by the compact recovery — that \
         would deliver it a second time, got {sent:?}"
    );
    assert_eq!(
        ix.store().send(send.id).await.unwrap().unwrap().status,
        SendStatus::Dispatched,
        "the compact recovery leaves the consumed row for the turn end to settle",
    );
}
