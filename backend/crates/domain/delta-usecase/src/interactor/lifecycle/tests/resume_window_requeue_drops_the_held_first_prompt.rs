//! A message requeued out of the resume window is typed EXACTLY once.
//!
//! Inside the resume window a send's row is written and the turn machine is
//! `AwaitingEcho`, but its keystrokes are deliberately held on the resuming
//! entry until `claude` accepts input. A prompt arriving before that cannot be
//! the held send's, so the machine requeues it — and from then on the `queued`
//! row is the message's single owner.
//!
//! The held copy has to go with it. While it stayed, the settle typed the held
//! text AND the next idle flush dispatched the requeued row: one composed
//! message, delivered twice. Dropping it makes the settle take its "no held
//! first prompt; flushing any queued send" branch instead, so the row travels
//! the ordinary queued path once.

use std::time::Instant;

use delta_model::{SendStatus, SessionId};

use crate::interactor::session_actor::runtime::RESUME_DISPATCH_SETTLE;
use crate::interactor::testing::*;
use crate::ports::StopHook;

#[tokio::test]
async fn resume_window_requeue_drops_the_held_first_prompt() {
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

    let (held, _) = ix
        .enqueue_send(to(main), "deliver me once", None)
        .await
        .unwrap();
    assert!(
        ix.tmux_fake().sent.lock().unwrap().is_empty(),
        "the first prompt is held until the resume is ready",
    );

    // Something submits a prompt while the window is still open — the resumed
    // session replaying its own state, or a human at the pane. It cannot be the
    // held send's, so that send goes back to the queue.
    ix.transcript_fake()
        .push(user_line("u-ext", "typed at the pane"));
    ix.on_user_prompt_submit(submit_in(
        "sess-R",
        "/elsewhere/t.jsonl",
        "/elsewhere",
        "typed at the pane",
    ))
    .await
    .unwrap();
    assert_eq!(
        ix.store().send(held.id).await.unwrap().unwrap().status,
        SendStatus::Queued,
        "the untyped send returned to the queue",
    );

    // The resume becomes ready and settles. With the held copy dropped, the
    // settle types nothing: it takes the no-held-prompt branch, whose queued
    // flush defers while the pane's own turn is still in flight.
    ix.on_session_start(session_start("sess-R", "resume"))
        .await
        .unwrap();
    ix.dispatch_ready_resumes(Instant::now() + RESUME_DISPATCH_SETTLE)
        .await
        .unwrap();
    assert!(
        ix.tmux_fake().sent.lock().unwrap().is_empty(),
        "the settle must not type a send the queue now owns",
    );

    // The pane's turn ends, and the queued row dispatches through the ordinary
    // turn-end trigger — the message's one and only delivery.
    ix.on_stop(StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();
    let sent = ix.tmux_fake().sent.lock().unwrap().clone();
    assert_eq!(
        sent.len(),
        1,
        "the requeued message is typed exactly once, got {sent:?}"
    );
    assert_eq!(sent[0].1, "deliver me once");
    assert_eq!(
        ix.store().send(held.id).await.unwrap().unwrap().status,
        SendStatus::Dispatched,
    );
}
