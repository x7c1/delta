//! The boot-time send restore and its explicit release: a `dispatched` row
//! left behind by a dead process is restored at startup — visible, never
//! auto-sent — and only an explicit release returns it to the normal queued
//! flow.
//!
//! Two dogfooding findings shaped this (2026-07-06, revised on-hardware):
//!
//! - The original zombie: turn state is runtime-only and rebuilt `Idle` on
//!   boot, but `send` rows are persistent. A row still `dispatched` when the
//!   server died had no turn machine awaiting its echo after the restart, yet
//!   — as the oldest `dispatched` row — it shadowed `head_dispatched_send`
//!   for every later `UserPromptSubmit`: an infinite requeue loop. The
//!   composition root now runs `SessionStore::restore_all_dispatched` once at
//!   boot, restoring the persistent half of the single-outstanding invariant.
//! - The auto-resend rejection: the first fix requeued the orphan, which made
//!   it silently re-submit on the next reopen — a message composed before a
//!   restart (possibly days old) landing in a conversation that has moved on,
//!   even *after* a newer message the user just sent. So a restored row is
//!   now excluded from every automatic dispatch trigger and waits for the
//!   user's explicit Send (release) or Cancel.

use std::time::Instant;

use delta_model::{SendStatus, SessionId};

use crate::error::Error;
use crate::interactor::session_actor::runtime::RESUME_DISPATCH_SETTLE;
use crate::interactor::testing::*;
use crate::ports::{NewSession, SessionEvent};

/// Seed the store as a dead process left it: the session registered and one
/// send still `dispatched`, with no actor (and so no turn state) anywhere.
async fn seed_pre_restart_store(ix: &TestInteractor) -> delta_model::Send {
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
        .enqueue_send(&session.id, main, None, "stale prompt", None)
        .await
        .unwrap();
    assert_eq!(stale.status, SendStatus::Dispatched);
    stale
}

/// The full restored-send lifecycle across a restart: the boot restore marks
/// the orphan, reopening the session types **nothing** (restore is not
/// resend — the settle flush skips the restored row), and only the explicit
/// release dispatches it through the normal queued path, where its echo
/// resolves it `matched` with no external-input misclassification.
#[tokio::test]
async fn boot_restored_send_stays_unsent_until_released_then_matches() {
    let ix = interactor();
    let session = SessionId::from("sess-1");
    let stale = seed_pre_restart_store(&ix).await;

    // What the composition root runs at startup: the one stale row restores.
    assert_eq!(ix.store().restore_all_dispatched().await.unwrap(), 1);
    let restored = ix.store().send(stale.id).await.unwrap().unwrap();
    assert_eq!(restored.status, SendStatus::Queued);
    assert!(restored.held_at.is_some());

    // The session reopens after the restart and its resume settles — the
    // trigger that used to auto-resend the row.
    ix.open_session(&session).await.unwrap();
    ix.on_session_start(session_start("sess-1", "resume"))
        .await
        .unwrap();
    let events = ix
        .dispatch_ready_resumes(Instant::now() + RESUME_DISPATCH_SETTLE)
        .await
        .unwrap();
    assert!(
        events.is_empty(),
        "the settle flush must not promote a restored row"
    );
    assert!(
        ix.tmux_fake().sent.lock().unwrap().is_empty(),
        "no keystroke is typed for a restored send on reopen"
    );
    let still = ix.store().send(stale.id).await.unwrap().unwrap();
    assert_eq!(still.status, SendStatus::Queued);
    assert!(still.held_at.is_some(), "the marker survives the settle");

    // The explicit release: the marker clears and — the session being open
    // and idle — the row dispatches immediately through the normal queued
    // path, reporting the transition for broadcast.
    let events = ix.release_send(stale.id).await.unwrap();
    assert!(
        events.iter().any(|e| matches!(
            e,
            SessionEvent::SendDispatched { send_id, .. } if *send_id == stale.id
        )),
        "the release reports the queued→dispatched transition"
    );
    let sent = ix.tmux_fake().sent.lock().unwrap().clone();
    assert_eq!(sent.len(), 1, "the released row types exactly once");
    assert_eq!(sent[0].1, "stale prompt");
    assert_eq!(
        ix.store().send(stale.id).await.unwrap().unwrap().status,
        SendStatus::Dispatched,
    );

    // Its echo arrives and MATCHES — before the boot restore existed, the
    // zombie stayed `dispatched` with no turn machine awaiting it, so this
    // hook classified every prompt as external input and looped.
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
        "the released send's echo correlates; it is not external input"
    );
    assert_eq!(
        ix.store().send(stale.id).await.unwrap().unwrap().status,
        SendStatus::Matched,
    );
}

/// Releasing while the session is still *closed* — the normal state right
/// after the restart that created the restored row — resumes it as part of
/// the release: `ensure_open` spawns `claude --resume <id>` (exactly as an
/// enqueue against a closed session would), the marker clears, and nothing
/// types during the resume-readiness window. Once the resume settles, the
/// settle flush picks the now-ordinary queued row up and types it exactly
/// once; its echo resolves it `matched`. Before the release routed through
/// ensure-open, this flow was a dead end: the marker cleared, the tail
/// dispatch no-opped (no live pane), and nothing ever resumed the session —
/// the row stranded `queued` forever.
#[tokio::test]
async fn release_on_a_closed_session_resumes_it_then_dispatches_at_settle() {
    let ix = interactor();
    let stale = seed_pre_restart_store(&ix).await;
    assert_eq!(ix.store().restore_all_dispatched().await.unwrap(), 1);

    // The explicit release, with no live pane anywhere (fresh server start,
    // session never reopened). No dispatch can happen yet, so no event.
    let events = ix.release_send(stale.id).await.unwrap();
    assert!(
        events.is_empty(),
        "nothing dispatches while the resume window is open"
    );

    // The release resumed the session: a `claude --resume sess-1` spawn was
    // recorded in the stored cwd, with no prior explicit open_session call.
    let created = ix.tmux_fake().created.lock().unwrap().clone();
    let resume = created
        .iter()
        .find(|c| c.command.iter().any(|a| a == "--resume"))
        .expect("the release resumed the closed session");
    assert_eq!(
        resume.command.last().map(String::as_str),
        Some("sess-1"),
        "resumes this session's conversation"
    );
    assert_eq!(resume.workdir, "/work", "resumes in the stored cwd");
    assert!(
        ix.tmux_fake().sent.lock().unwrap().is_empty(),
        "no keystroke may be typed while the resume window is open"
    );

    // The marker cleared: the row is an ordinary queued send now, waiting
    // for the settle flush.
    let released = ix.store().send(stale.id).await.unwrap().unwrap();
    assert_eq!(released.status, SendStatus::Queued);
    assert!(
        released.held_at.is_none(),
        "the release cleared the restored marker"
    );

    // The resume becomes ready and settles: the flush promotes the released
    // row through the normal queued path and types it exactly once.
    ix.on_session_start(session_start("sess-1", "resume"))
        .await
        .unwrap();
    let events = ix
        .dispatch_ready_resumes(Instant::now() + RESUME_DISPATCH_SETTLE)
        .await
        .unwrap();
    assert!(
        events.iter().any(|e| matches!(
            e,
            SessionEvent::SendDispatched { send_id, .. } if *send_id == stale.id
        )),
        "the settle flush reports the queued→dispatched transition"
    );
    let sent = ix.tmux_fake().sent.lock().unwrap().clone();
    assert_eq!(sent.len(), 1, "the released row types exactly once");
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
        "the released send's echo correlates; it is not external input"
    );
    assert_eq!(
        ix.store().send(stale.id).await.unwrap().unwrap().status,
        SendStatus::Matched,
    );
}

/// An ensure-open failure surfaces from the release *before* the marker is
/// touched: with the session's transcript gone, `claude --resume` is
/// impossible, so the release fails `ResumeUnavailable`, spawns nothing, and
/// leaves the row still restored — the user can cancel it, or retry the
/// release once the transcript is back.
#[tokio::test]
async fn release_on_an_unresumable_session_fails_and_keeps_the_marker() {
    let ix = interactor();
    let stale = seed_pre_restart_store(&ix).await;
    assert_eq!(ix.store().restore_all_dispatched().await.unwrap(), 1);
    ix.transcript_fake().mark_missing(SEED_TRANSCRIPT_PATH);

    assert!(matches!(
        ix.release_send(stale.id).await,
        Err(Error::ResumeUnavailable(id)) if id == "sess-1"
    ));
    assert!(
        ix.tmux_fake().created.lock().unwrap().is_empty(),
        "a refused resume spawns no pane"
    );
    let row = ix.store().send(stale.id).await.unwrap().unwrap();
    assert_eq!(row.status, SendStatus::Queued);
    assert!(
        row.held_at.is_some(),
        "the failed release leaves the row restored, so it can be retried"
    );
}

/// A release only takes effect on a still-queued restored row: a duplicate
/// release, a never-restored queued row, and an unknown id are all clean
/// `SendNotReleasable` conflicts — mirroring the cancel path's guarded
/// transitions so a race never clobbers state.
#[tokio::test]
async fn release_conflicts_are_send_not_releasable() {
    let ix = interactor();
    let session = SessionId::from("sess-1");
    let stale = seed_pre_restart_store(&ix).await;
    assert_eq!(ix.store().restore_all_dispatched().await.unwrap(), 1);

    ix.open_session(&session).await.unwrap();
    ix.on_session_start(session_start("sess-1", "resume"))
        .await
        .unwrap();
    ix.dispatch_ready_resumes(Instant::now() + RESUME_DISPATCH_SETTLE)
        .await
        .unwrap();

    // First release wins; the second finds the marker already cleared (the
    // row has even dispatched by now).
    ix.release_send(stale.id).await.unwrap();
    assert!(matches!(
        ix.release_send(stale.id).await,
        Err(Error::SendNotReleasable(id)) if id == stale.id
    ));

    // A genuinely queued row (never restored) is not releasable.
    let main = ix.store().main_thread_id(&session).await.unwrap();
    let plain = ix
        .store()
        .enqueue_queued_send(&session, main, None, "plain queued", None)
        .await
        .unwrap();
    assert!(matches!(
        ix.release_send(plain.id).await,
        Err(Error::SendNotReleasable(id)) if id == plain.id
    ));

    // An unknown id conflicts before any actor is involved.
    assert!(matches!(
        ix.release_send(9999).await,
        Err(Error::SendNotReleasable(9999))
    ));
}

/// Cancel keeps covering restored rows: a restored send's status is still
/// `queued`, so the existing guarded queued cancel drops it — and a release
/// after that cancel is a clean conflict, not a resurrection.
#[tokio::test]
async fn restored_send_is_cancellable_and_not_releasable_after_cancel() {
    let ix = interactor();
    let stale = seed_pre_restart_store(&ix).await;
    assert_eq!(ix.store().restore_all_dispatched().await.unwrap(), 1);

    ix.cancel_send(stale.id).await.unwrap();
    assert_eq!(
        ix.store().send(stale.id).await.unwrap().unwrap().status,
        SendStatus::Cancelled,
    );
    assert!(matches!(
        ix.release_send(stale.id).await,
        Err(Error::SendNotReleasable(id)) if id == stale.id
    ));
}
