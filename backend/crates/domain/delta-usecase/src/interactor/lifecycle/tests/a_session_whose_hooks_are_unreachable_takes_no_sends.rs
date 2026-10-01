//! A re-adopted session whose agent can no longer reach this server's hooks
//! takes no sends: typing into its pane would land, but with no
//! `UserPromptSubmit` echo the echo deadline would inject `Escape` and type the
//! prompt again, interrupting the running turn and submitting it twice. Closing
//! the session — what the browser's notice asks for — is the way back: the next
//! send resumes it with current settings.

use crate::error::Error;
use crate::interactor::testing::*;

use super::readoption_support::{left_behind, listing, survives};

#[tokio::test]
async fn a_send_into_a_session_whose_hooks_are_unreachable_is_refused() {
    let ix = interactor();
    let id = left_behind(&ix, "sess-A", "delta-7").await;
    survives(&ix, "delta-7");
    ix.readopt_surviving_sessions(true).await.unwrap();
    let main = ix.store().main_thread_id(&id).await.unwrap();

    let result = ix.enqueue_send(to(main), "next", None).await;

    assert!(
        matches!(result, Err(Error::SessionHooksUnreachable(ref sid)) if sid == id.as_str()),
        "refused with the typed error, got {result:?}"
    );
    assert!(
        ix.tmux_fake().sent.lock().unwrap().is_empty(),
        "nothing was typed into the pane"
    );
    assert!(
        ix.store().open_sends(&id).await.unwrap().is_empty(),
        "no send row was written"
    );
    // The session stays open on its pane, still showing the notice.
    let row = listing(&ix, &id).await;
    assert!(row.open);
    assert!(row.hooks_unreachable);
}

/// Releasing a held send types it just like a send does, so it is refused the
/// same way, and the row stays held for after the session has been closed.
#[tokio::test]
async fn releasing_a_held_send_into_such_a_session_is_refused_and_keeps_it_held() {
    let ix = interactor();
    let id = left_behind(&ix, "sess-A", "delta-7").await;
    let main = ix.store().main_thread_id(&id).await.unwrap();
    let held = ix
        .store()
        .enqueue_send(&id, main, None, "never seen by the pane", None)
        .await
        .unwrap();
    ix.store().restore_all_dispatched().await.unwrap();
    survives(&ix, "delta-7");
    ix.readopt_surviving_sessions(true).await.unwrap();

    let result = ix.release_send(held.id).await;

    assert!(
        matches!(result, Err(Error::SessionHooksUnreachable(_))),
        "refused with the typed error, got {result:?}"
    );
    assert!(ix.tmux_fake().sent.lock().unwrap().is_empty());
    let row = ix.store().send(held.id).await.unwrap().expect("row kept");
    assert!(
        row.held_at.is_some(),
        "still held for the user to send later"
    );
}

/// A queued row of such a session is not flushed into the pane either: the
/// queue's flush types it just as a fresh send would be typed. With no hooks
/// arriving, the transcript tail is what still drives the queue's triggers.
#[tokio::test]
async fn a_queued_send_of_such_a_session_is_not_flushed_into_its_pane() {
    let ix = interactor();
    let id = left_behind(&ix, "sess-A", "delta-7").await;
    let main = ix.store().main_thread_id(&id).await.unwrap();
    let queued = ix
        .store()
        .enqueue_queued_send(&id, main, None, "composed mid-turn", None)
        .await
        .unwrap();
    survives(&ix, "delta-7");
    ix.readopt_surviving_sessions(true).await.unwrap();

    // A turn ends in the transcript (an API error writes no hook either way),
    // which is a queue-flush trigger.
    ix.transcript_fake()
        .push_to("/work/sess-A.jsonl", api_error_line("api-error"));
    ix.poll_transcript(TICK_BOUND).await.unwrap();

    assert!(ix.tmux_fake().sent.lock().unwrap().is_empty());
    let row = ix.store().send(queued.id).await.unwrap().expect("row kept");
    assert_eq!(row.status, delta_model::SendStatus::Queued);
}

/// A send that reaches a closed session whose remembered pane is still alive
/// and marked adopts that pane (the resume backstop), and is then refused
/// rather than typed: the agent in it is the one that cannot reach the hooks.
#[tokio::test]
async fn a_send_that_adopts_a_marked_pane_through_the_backstop_is_refused() {
    let ix = interactor();
    let id = left_behind(&ix, "sess-A", "delta-7").await;
    survives(&ix, "delta-7");
    ix.tmux_fake().fail_probes();
    ix.readopt_surviving_sessions(true).await.unwrap();
    *ix.tmux_fake().probe_fails.lock().unwrap() = false;
    let main = ix.store().main_thread_id(&id).await.unwrap();

    let result = ix.enqueue_send(to(main), "next", None).await;

    assert!(matches!(result, Err(Error::SessionHooksUnreachable(_))));
    assert!(ix.tmux_fake().created.lock().unwrap().is_empty());
    assert!(ix.tmux_fake().sent.lock().unwrap().is_empty());
    assert!(ix.store().open_sends(&id).await.unwrap().is_empty());
    // Open on the adopted pane now, so the browser's next list shows the notice.
    let row = listing(&ix, &id).await;
    assert!(row.open);
    assert!(row.hooks_unreachable);
}

/// Close, then send: the send resumes the session with fresh settings, which is
/// what the notice tells the user to do.
#[tokio::test]
async fn after_close_a_send_resumes_the_session_again() {
    let ix = interactor();
    let id = left_behind(&ix, "sess-A", "delta-7").await;
    survives(&ix, "delta-7");
    ix.readopt_surviving_sessions(true).await.unwrap();
    let main = ix.store().main_thread_id(&id).await.unwrap();
    assert!(matches!(
        ix.enqueue_send(to(main), "first", None).await,
        Err(Error::SessionHooksUnreachable(_))
    ));

    ix.close_session(&id).await.unwrap();
    let send = ix.enqueue_send(to(main), "again", None).await.unwrap();

    assert_eq!(send.0.text, "again");
    assert_eq!(
        ix.tmux_fake().created.lock().unwrap().len(),
        1,
        "the send resumed the session into a fresh pane"
    );
    assert!(!listing(&ix, &id).await.hooks_unreachable);
}

/// The mark belongs to the pane, not to the session: a closed session whose
/// marked record names a pane that has since died is not refused. The send's
/// resume backstop finds the pane gone, and the session resumes into a fresh
/// pane whose agent reaches the current hooks.
#[tokio::test]
async fn a_marked_record_whose_pane_died_does_not_refuse_the_send() {
    let ix = interactor();
    let id = left_behind(&ix, "sess-A", "delta-7").await;
    // Boot could not ask tmux, so the changed endpoint was recorded on the
    // pane it may still be running in.
    ix.tmux_fake().fail_probes();
    ix.readopt_surviving_sessions(true).await.unwrap();
    *ix.tmux_fake().probe_fails.lock().unwrap() = false;
    assert!(
        ix.store()
            .remembered_pane(&id)
            .await
            .unwrap()
            .expect("record kept")
            .hooks_unreachable
    );
    let main = ix.store().main_thread_id(&id).await.unwrap();

    // `delta-7` is not running: the pane died while nothing was looking.
    let send = ix.enqueue_send(to(main), "next", None).await.unwrap();

    assert_eq!(send.0.text, "next");
    let created = ix.tmux_fake().created.lock().unwrap().clone();
    assert_eq!(created.len(), 1, "resumed into a fresh pane");
    let remembered = ix
        .store()
        .remembered_pane(&id)
        .await
        .unwrap()
        .expect("the fresh pane is remembered");
    assert_eq!(remembered.tmux_session, created[0].name);
    assert!(!remembered.hooks_unreachable);
    assert!(!listing(&ix, &id).await.hooks_unreachable);
}
