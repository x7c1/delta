//! The one exception to positional consumption: the resume window.
//!
//! Consumption assumes the outstanding send's keystrokes are *in the pane* —
//! which is what makes "a prompt arrived, so it must be that send's" sound.
//! A send that resumes a closed session breaks that assumption: its keystrokes
//! are deliberately held until `SessionStart(source=resume)` says `claude`
//! accepts input, so a prompt arriving during the window cannot be theirs. The
//! held send must survive it — still deliverable, never settled as delivered.
//! (The same reasoning keeps the echo-deadline sweep off a held send.)

use delta_model::{SendStatus, SessionId};

use crate::interactor::testing::*;
use crate::ports::SessionEvent;
use crate::turn::TurnState;

#[tokio::test]
async fn resume_window_prompt_does_not_consume_the_held_send() {
    let ix = interactor();
    // A known-but-closed session: the send resumes it and holds the keystroke.
    ix.on_user_prompt_submit(submit_in(
        "sess-R",
        "/elsewhere/t.jsonl",
        "/elsewhere",
        "seed",
    ))
    .await
    .unwrap();
    let session = SessionId::from("sess-R");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    let (send, _) = ix
        .enqueue_send(to(main), "after resume", None)
        .await
        .unwrap();
    assert!(
        ix.tmux_fake().sent.lock().unwrap().is_empty(),
        "the first prompt is held until the resume is ready",
    );
    assert_eq!(
        ix.live_state_for(&session).await.turn,
        TurnState::AwaitingEcho { send_id: send.id },
    );

    // Something submits a prompt while the window is still open — the resumed
    // session replaying its own state, or a human at the pane. It cannot be the
    // held send, whose text has not been typed anywhere yet.
    ix.transcript_fake()
        .push(user_line("u-ext", "typed at the pane"));
    let (events, _) = ix
        .on_user_prompt_submit(submit_in(
            "sess-R",
            "/elsewhere/t.jsonl",
            "/elsewhere",
            "typed at the pane",
        ))
        .await
        .unwrap();

    // The turn belongs to that prompt, not to the held send…
    assert_eq!(
        ix.live_state_for(&session).await.turn,
        TurnState::InFlight { send_id: None },
        "the held send did not consume this prompt",
    );
    assert!(
        events.iter().any(|e| matches!(
            e,
            SessionEvent::ExternalInput { prompt, .. } if prompt == "typed at the pane"
        )),
        "a prompt that consumed no send is external input; got {events:?}"
    );

    // …and the send is still a message waiting to be delivered: returned to the
    // queue for the next idle dispatch, never recorded as delivered.
    let held = ix.store().send(send.id).await.unwrap().unwrap();
    assert_eq!(
        held.status,
        SendStatus::Queued,
        "an untyped send goes back to the queue, never settled as delivered",
    );
}
