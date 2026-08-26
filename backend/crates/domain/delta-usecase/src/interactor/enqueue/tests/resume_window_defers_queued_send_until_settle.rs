//! The resume window's interplay with queued sends, pinning two gaps observed
//! after the boot-time send reconcile landed (dogfooding, 2026-07-06):
//!
//! - **No flush at resume settle**: a genuinely `queued` row (e.g. one
//!   composed mid-turn that survived a restart as `queued`) never
//!   re-dispatched when its session was merely reopened — none of the
//!   queued-dispatch triggers (turn end, interrupt ingest, enqueue idle-flush)
//!   fire on a plain resume, so the row sat pinned until the user happened to
//!   send another message. `dispatch_ready_resume` now flushes the queued
//!   send when a settled resume holds no first prompt.
//! - **The idle-flush bypassed the resume gate**: enqueueing while the resume
//!   window was still open promoted the queued row and typed its keystrokes
//!   into the freshly-bound pane before `claude` accepted input — the
//!   keystrokes were silently lost and the row stuck in `dispatched` awaiting
//!   an echo that could never arrive. `dispatch_queued_send` is now a no-op
//!   while the resume window is open; the deferred row is flushed at settle.
//!
//! *Restored* rows — recovered at boot from a dead process's `dispatched`
//! state — are deliberately NOT part of this flush: they never dispatch
//! automatically (see `boot_restored_send_awaits_explicit_release`); the
//! companion test at the bottom pins the settle flush skipping them.

use std::time::Instant;

use delta_model::{SendStatus, SessionId};

use crate::interactor::session_actor::runtime::RESUME_DISPATCH_SETTLE;
use crate::interactor::testing::*;
use crate::ports::{NewSession, SessionEvent, StopHook};

/// Seed the store as a restart leaves a *genuinely queued* send: `sess-1`
/// known but closed (no actor, no pane, turn implicitly idle) with one
/// `queued`, unrestored row — e.g. a message composed while a turn was in
/// flight that the dead process never got to dispatch.
async fn seed_closed_session_with_queued_send(ix: &TestInteractor) -> delta_model::Send {
    let (session, main) = ix
        .store()
        .register_session(NewSession {
            id: "sess-1".into(),
            cwd: "/work".into(),
            transcript_path: SEED_TRANSCRIPT_PATH.into(),
            branch_at_launch: None,
            repo_root: None,
            repository_display_name: None,
        })
        .await
        .unwrap();
    let stale = ix
        .store()
        .enqueue_queued_send(&session.id, main, None, "stale prompt", None)
        .await
        .unwrap();
    assert_eq!(stale.status, SendStatus::Queued);
    assert_eq!(stale.held_at, None, "genuinely queued, not restored");
    stale
}

/// While the resume window is still open (the pane is bound but the resume
/// has not settled), no trigger may type the queued row: its keystrokes would
/// land in a pane not yet accepting input and be silently lost, wedging the
/// row in `dispatched`. Enqueueing a new message during the window reaches
/// `enqueue_into_open`'s idle-flush — which previously did exactly that — so
/// this pins the flush deferring instead: nothing reaches the tmux driver and
/// the row stays `queued`.
#[tokio::test]
async fn queued_send_stays_queued_while_the_resume_window_is_open() {
    let ix = interactor();
    let session = SessionId::from("sess-1");
    let stale = seed_closed_session_with_queued_send(&ix).await;

    // Reopen after the restart: the pane binds immediately, but the session
    // sits inside the resume-readiness window (resuming entry present, turn
    // idle).
    ix.open_session(&session).await.unwrap();

    // A new message composed while the window is still open.
    let main = ix.store().main_thread_id(&session).await.unwrap();
    let (held, _) = ix.enqueue_send(to(main), "new prompt", None).await.unwrap();

    assert!(
        ix.tmux_fake().sent.lock().unwrap().is_empty(),
        "no keystroke may be typed while the resume window is open"
    );
    assert_eq!(
        ix.store().send(stale.id).await.unwrap().unwrap().status,
        SendStatus::Queued,
        "the queued row is deferred, not promoted, during the resume window"
    );
    // The new prompt took the held-first-prompt slot: its row is written and
    // outstanding, only its keystroke is held until the settle.
    assert_eq!(held.status, SendStatus::Dispatched);
}

/// The flush the settle promises for a genuinely queued row: reopening the
/// session dispatches it at resume settle. Nothing types during the window;
/// once `SessionStart(resume)` has marked the resume ready and the settle
/// elapses, the row is promoted to `dispatched`, typed exactly once, and the
/// `SendDispatched` event is returned for broadcast. Its echo then resolves
/// it `matched` through the normal correlation.
#[tokio::test]
async fn resume_settle_with_no_held_prompt_flushes_the_queued_send() {
    let ix = interactor();
    let session = SessionId::from("sess-1");
    let stale = seed_closed_session_with_queued_send(&ix).await;

    ix.open_session(&session).await.unwrap();
    ix.on_session_start(session_start("sess-1", "resume"))
        .await
        .unwrap();
    assert!(
        ix.tmux_fake().sent.lock().unwrap().is_empty(),
        "readiness only marks the resume; nothing types before the settle"
    );

    // The settle tick: no held first prompt, so the flush releases the queued
    // row.
    let events = ix
        .dispatch_ready_resumes(Instant::now() + RESUME_DISPATCH_SETTLE)
        .await
        .unwrap();
    assert!(
        events.iter().any(|e| matches!(
            e,
            SessionEvent::SendDispatched { send_id, .. } if *send_id == stale.id
        )),
        "the flush reports the queued→dispatched transition for broadcast"
    );
    let sent = ix.tmux_fake().sent.lock().unwrap().clone();
    assert_eq!(sent.len(), 1, "the queued row types exactly once");
    assert_eq!(sent[0].1, "stale prompt");
    assert_eq!(
        ix.store().send(stale.id).await.unwrap().unwrap().status,
        SendStatus::Dispatched,
    );

    // Its echo arrives and correlates: the send resolves `matched`, with no
    // external-input misclassification.
    ix.transcript_fake()
        .push(user_line("u-stale", "stale prompt"));
    let (events, _) = ix
        .on_user_prompt_submit(submit("stale prompt"))
        .await
        .unwrap();
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, SessionEvent::ExternalInput { .. })),
        "the flushed send's echo correlates; it is not external input"
    );
    assert_eq!(
        ix.store().send(stale.id).await.unwrap().unwrap().status,
        SendStatus::Matched,
    );
}

/// A held first prompt wins the settle: it types first (its turn machine is
/// already `AwaitingEcho`, so the settle performs no queued-send flush), and
/// the pre-existing queued row stays `queued` until that prompt's turn ends —
/// the turn-end trigger then dispatches it through the normal chain.
#[tokio::test]
async fn resume_settle_with_held_prompt_types_it_first_queued_row_follows_on_turn_end() {
    let ix = interactor();
    let session = SessionId::from("sess-1");
    let stale = seed_closed_session_with_queued_send(&ix).await;

    ix.open_session(&session).await.unwrap();
    let main = ix.store().main_thread_id(&session).await.unwrap();
    let (held, _) = ix.enqueue_send(to(main), "new prompt", None).await.unwrap();

    ix.on_session_start(session_start("sess-1", "resume"))
        .await
        .unwrap();
    let events = ix
        .dispatch_ready_resumes(Instant::now() + RESUME_DISPATCH_SETTLE)
        .await
        .unwrap();
    assert!(
        events.is_empty(),
        "the held-prompt dispatch is not a queued-send promotion"
    );
    let sent = ix.tmux_fake().sent.lock().unwrap().clone();
    assert_eq!(sent.len(), 1, "only the held first prompt types at settle");
    assert_eq!(sent[0].1, "new prompt");
    assert_eq!(
        ix.store().send(stale.id).await.unwrap().unwrap().status,
        SendStatus::Queued,
        "the queued row waits behind the held prompt's turn"
    );

    // The held prompt's turn runs (echo) and completes (Stop): the turn-end
    // trigger now dispatches the queued row.
    ix.transcript_fake().push(user_line("u-new", "new prompt"));
    ix.on_user_prompt_submit(submit("new prompt"))
        .await
        .unwrap();
    assert_eq!(
        ix.store().send(held.id).await.unwrap().unwrap().status,
        SendStatus::Matched,
    );
    ix.on_stop(StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();
    let sent = ix.tmux_fake().sent.lock().unwrap().clone();
    assert_eq!(
        sent.len(),
        2,
        "the queued row dispatches when the held prompt's turn ends"
    );
    assert_eq!(sent[1].1, "stale prompt");
    assert_eq!(
        ix.store().send(stale.id).await.unwrap().unwrap().status,
        SendStatus::Dispatched,
    );
}

/// The settle flush is selective: with a *restored* row (boot-recovered from
/// a dead process's `dispatched` state) queued ahead of a genuinely queued
/// row, the settle types only the genuinely queued one. The restored row —
/// even though it is older and would win FIFO if it were eligible — stays
/// queued and marked, awaiting its explicit release.
#[tokio::test]
async fn resume_settle_flushes_the_queued_row_but_never_a_restored_one() {
    let ix = interactor();
    let session = SessionId::from("sess-1");

    // The restored row first (older), the genuinely queued row behind it.
    let (registered, main) = ix
        .store()
        .register_session(NewSession {
            id: "sess-1".into(),
            cwd: "/work".into(),
            transcript_path: SEED_TRANSCRIPT_PATH.into(),
            branch_at_launch: None,
            repo_root: None,
            repository_display_name: None,
        })
        .await
        .unwrap();
    let restored = ix
        .store()
        .enqueue_send(&registered.id, main, None, "restored prompt", None)
        .await
        .unwrap();
    assert_eq!(ix.store().restore_all_dispatched().await.unwrap(), 1);
    let queued = ix
        .store()
        .enqueue_queued_send(&registered.id, main, None, "queued prompt", None)
        .await
        .unwrap();

    ix.open_session(&session).await.unwrap();
    ix.on_session_start(session_start("sess-1", "resume"))
        .await
        .unwrap();
    let events = ix
        .dispatch_ready_resumes(Instant::now() + RESUME_DISPATCH_SETTLE)
        .await
        .unwrap();

    // Only the genuinely queued row flushes; the older restored row is
    // skipped, not typed, and keeps its marker.
    assert!(
        events.iter().any(|e| matches!(
            e,
            SessionEvent::SendDispatched { send_id, .. } if *send_id == queued.id
        )),
        "the settle flush promotes the genuinely queued row"
    );
    let sent = ix.tmux_fake().sent.lock().unwrap().clone();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].1, "queued prompt");
    let restored = ix.store().send(restored.id).await.unwrap().unwrap();
    assert_eq!(restored.status, SendStatus::Queued);
    assert!(
        restored.held_at.is_some(),
        "the restored row survives the settle untouched"
    );
}
