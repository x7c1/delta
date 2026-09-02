//! Interrupting a terminal-less (Codex) session, end to end through the
//! interactor's actor path.
//!
//! An interrupt must abort the in-flight turn *without* closing the session:
//! unlike a close, it reaches the open agent non-destructively (leaving the
//! event pump alive) so the provider's `turn/completed{interrupted}` still
//! arrives and settles the turn. This test drives the whole path:
//!
//! 1. Stand up a Codex session with a first prompt — its turn is in flight and
//!    its event pump is draining the adapter.
//! 2. `interrupt(session_id)` (the REST path's entry) routes to the owning
//!    session's actor, which reaches the open agent and drives the adapter's
//!    `interrupt` over the trait.
//! 3. The adapter emits `TurnCompleted{Interrupted}` on the stream (as the real
//!    Codex provider does on `turn/interrupt`); the pump ingests it and drives
//!    the turn machine back to `Idle` via `apply_turn_end(Interrupted)`.
//!
//! The session stays open throughout — the open agent (and its pump) is never
//! torn down — which is the deliberate difference from `close_session`.

use std::time::Duration;

use delta_model::AgentProvider;

use crate::interactor::testing::*;
use crate::turn::TurnState;
use crate::SendTarget;

/// Poll `f` until it returns `Some`, or panic after a short deadline. The event
/// pump runs on a background task, so the turn-end it drives lands
/// asynchronously; this yields to it between checks.
async fn wait_for<T, F, Fut>(what: &str, mut f: F) -> T
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Option<T>>,
{
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(value) = f().await {
            return value;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("timed out waiting for {what}");
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

#[tokio::test]
async fn interrupt_reaches_the_adapter_and_keeps_the_session_open() {
    let factory = FakeAgentFactory::new("thr_interrupt", Some("turn_interrupt"));
    let log = factory.log();
    let ix = interactor_with_codex_factory(factory);

    // Stand up a Codex session (its event pump starts draining the adapter).
    let (send, _) = ix
        .enqueue_send(
            SendTarget::NewSession {
                pull_request_number: None,
                provider: AgentProvider::Codex,
                workdir: None,
                launch_option_ids: Vec::new(),
                worktree: None,
            },
            "long task",
            None,
        )
        .await
        .unwrap();
    ix.await_launch().await;
    let session_id = send.session_id.clone();

    // The turn is in flight (tracked as consuming no send, no send id).
    assert_eq!(
        ix.live_state_for(&session_id).await.turn,
        TurnState::InFlight { send_id: None },
    );

    // Interrupt the session (the REST path's entry).
    ix.interrupt(&session_id)
        .await
        .expect("interrupt routes to the Codex session");

    // The adapter received exactly one interrupt over the trait.
    assert_eq!(
        log.lock().unwrap().interrupts,
        1,
        "interrupt reached the adapter"
    );
    // The session was NOT closed by the interrupt: the open agent (and its pump)
    // survive, so `close` was never driven.
    assert_eq!(
        log.lock().unwrap().closes,
        0,
        "interrupt must not close the session (the pump must stay alive)"
    );

    // The interrupt-driven `turn/completed{interrupted}` flows back through the
    // still-live pump, which drives the turn machine to `Idle` via
    // `apply_turn_end(Interrupted)`.
    let sid = session_id.clone();
    wait_for("the interrupted turn to settle to idle", || {
        let ix = &ix;
        let sid = sid.clone();
        async move { matches!(ix.live_state_for(&sid).await.turn, TurnState::Idle).then_some(()) }
    })
    .await;

    // The session is still open after the interrupt settled — the pump was never
    // torn down (the whole point of interrupt vs. close).
    assert!(
        ix.is_session_open(&session_id).await,
        "the session stays open after an interrupt"
    );
}
