//! The boot-time send reconcile: a `dispatched` row left behind by a dead
//! process is requeued at startup, so it cannot shadow `UserPromptSubmit`
//! correlation after the restart.
//!
//! The bug this pins (observed in dogfooding, 2026-07-06): turn state is
//! runtime-only and rebuilt `Idle` on boot, but `send` rows are persistent. A
//! row still `dispatched` when the server died had no turn machine awaiting
//! its echo after the restart, yet — as the oldest `dispatched` row — it was
//! what `head_dispatched_send` returned for every later `UserPromptSubmit`.
//! Each new send then dispatched, mismatched against the zombie's text,
//! classified as external input, and was requeued by the turn machine's
//! anomaly path: an infinite requeue loop re-submitting the same prompt on
//! every turn end. The composition root now runs
//! `SessionStore::requeue_all_dispatched` once at boot (before any session
//! actor exists, when every `dispatched` row is an orphan by definition),
//! restoring the persistent half of the single-outstanding invariant.

use delta_model::{SendStatus, SessionId};

use crate::interactor::testing::*;
use crate::ports::{NewSession, SessionEvent, StopHook};
use crate::turn::TurnState;

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

/// The full head-of-line scenario across a restart: the reconcile requeues
/// the stale row, the reopened session re-dispatches it through the normal
/// idle path ahead of a newly composed send (FIFO), and both sends match
/// their echoes — no external-input misclassification, no requeue loop.
#[tokio::test]
async fn boot_reconcile_unblocks_correlation_after_a_restart() {
    let ix = interactor();
    let session = SessionId::from("sess-1");
    let stale = seed_pre_restart_store(&ix).await;

    // What the composition root runs at startup: the one stale row requeues.
    assert_eq!(ix.store().requeue_all_dispatched().await.unwrap(), 1);
    assert_eq!(
        ix.store().send(stale.id).await.unwrap().unwrap().status,
        SendStatus::Queued,
    );

    // The session reopens after the restart (register + turn end + a live,
    // ready pane).
    ix.seed_session().await;
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // A new send composed after the restart. The idle-flush releases the
    // stale (older) send first, so FIFO order is preserved: the stale prompt
    // re-types through the normal dispatch path and the new send queues
    // behind it.
    let (fresh, _) = ix
        .enqueue_send(to(main), "fresh prompt", None)
        .await
        .unwrap();
    assert_eq!(fresh.status, SendStatus::Queued);
    assert_eq!(
        ix.live_state_for(&session).await.turn,
        TurnState::AwaitingEcho { send_id: stale.id },
        "the requeued stale send re-dispatches first (FIFO)"
    );
    let sent = ix.tmux_fake().sent.lock().unwrap().clone();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].1, "stale prompt");

    // The stale send's echo arrives and MATCHES — before the fix the zombie
    // stayed `dispatched` with no turn machine awaiting it, so this hook
    // classified every prompt as external input and requeued the new send.
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
        "the re-dispatched send's echo correlates; it is not external input"
    );
    assert_eq!(
        ix.store().send(stale.id).await.unwrap().unwrap().status,
        SendStatus::Matched,
    );

    // Its turn ends: the new send dispatches through the same idle path and
    // resolves matched too. No requeue loop anywhere.
    ix.on_stop(StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();
    let sent = ix.tmux_fake().sent.lock().unwrap().clone();
    assert_eq!(sent.len(), 2, "the new send dispatches on the turn end");
    assert_eq!(sent[1].1, "fresh prompt");

    ix.transcript_fake()
        .push(user_line("u-fresh", "fresh prompt"));
    let (events, _) = ix
        .on_user_prompt_submit(submit("fresh prompt"))
        .await
        .unwrap();
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, SessionEvent::ExternalInput { .. })),
        "the new send's echo correlates too — no requeue loop"
    );
    assert_eq!(
        ix.store().send(fresh.id).await.unwrap().unwrap().status,
        SendStatus::Matched,
    );
    assert_eq!(
        ix.live_state_for(&session).await.turn,
        TurnState::InFlight {
            send_id: Some(fresh.id)
        },
    );
}
