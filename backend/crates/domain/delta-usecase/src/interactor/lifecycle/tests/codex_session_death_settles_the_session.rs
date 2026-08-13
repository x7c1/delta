//! An adapter-backed (Codex) session whose backing process dies must **settle**,
//! not go quiet.
//!
//! The field failure this pins down: a `codex app-server` process was killed
//! mid-turn. The transport noticed only its own reader hitting EOF, so nothing
//! in the session's runtime moved — the turn stayed `in_flight` forever, the
//! permission dialog stayed on screen (the user's Allow then failed to write,
//! and the retry answered `409`), and the session still reported itself open.
//! Indistinguishable from a hang.
//!
//! The adapter now surfaces the death as `SessionEnded { ProcessExited }` and the
//! pump settles everything it strands. These tests cover the operation × state
//! matrix at the use-case layer, where the exact state transitions and browser
//! signals are observable (the real-stack proof, over a killed `fake-codex`,
//! lives in `fake-codex/tests/full_loop.rs`):
//!
//! - death **mid-turn with pending approvals** → every dialog clears
//!   client-visibly, no row is left `pending` (each carries the reason it was
//!   denied), the turn ends as interrupted, the session closes, and a decision
//!   for a stranded request is a clean conflict rather than a write to a dead
//!   wire;
//! - death **mid-turn with nothing pending** → the same turn end and close, with
//!   no permission settle invented for a session that had none;
//! - a **send after a death** → the session resumes over a fresh process and runs
//!   a whole turn, because recovery is the existing resume path and never a
//!   respawn;
//! - death **while idle** → the session closes with no turn signal at all;
//! - an **orderly close** → unchanged: exactly one session-closed signal and no
//!   failure variant, even though the adapter's own `SessionEnded { Closed }`
//!   flows through the same pump.

use std::time::Duration;

use delta_model::{AgentProvider, PermissionStatus};
use serde_json::json;

use crate::agent::{AgentEvent, AgentPermissionRequest, SessionEndReason, TurnStatus};
use crate::interactor::testing::*;
use crate::interactor::PermissionDecision;
use crate::ports::{AsyncEventReceiver, SessionEvent};
use crate::turn::TurnState;
use crate::SendTarget;

/// A short bound so a wiring bug fails fast instead of hanging the suite.
const DEADLINE: Duration = Duration::from_secs(5);

/// Poll `f` until it returns `Some`, or panic after a short deadline. The event
/// pump runs on a background task, so its effect on the runtime lands
/// asynchronously; this yields to it between checks.
async fn wait_for<T, F, Fut>(what: &str, mut f: F) -> T
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Option<T>>,
{
    let deadline = tokio::time::Instant::now() + DEADLINE;
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

/// Drain the async seam until `stop` matches an event, returning everything
/// received up to and including it. The settle emits several events in one pass,
/// so a test asserts over the whole batch rather than event by event.
async fn drain_until(
    events: &mut AsyncEventReceiver,
    stop: impl Fn(&SessionEvent) -> bool,
) -> Vec<SessionEvent> {
    let mut received = Vec::new();
    let deadline = tokio::time::Instant::now() + DEADLINE;
    loop {
        let event = tokio::time::timeout_at(deadline, events.recv())
            .await
            .expect("timed out waiting for the settle's events")
            .expect("the async seam stayed open");
        let done = stop(&event);
        received.push(event);
        if done {
            return received;
        }
    }
}

/// Stand up a Codex session with a first prompt (its turn in flight, its event
/// pump draining the adapter), returning the interactor, the seam receiver, the
/// session id and the sender that drives the adapter's stream.
async fn codex_session_in_flight(
    thread_id: &str,
) -> (
    TestInteractor,
    AsyncEventReceiver,
    delta_model::SessionId,
    tokio::sync::mpsc::UnboundedSender<AgentEvent>,
) {
    let factory = FakeAgentFactory::new(thread_id, Some("turn_death"));
    let stream = factory.event_sender();
    let (ix, events) = interactor_with_codex_factory_and_event_sink(factory);
    let (send, _) = ix
        .enqueue_send(
            SendTarget::NewSession {
                provider: AgentProvider::Codex,
                workdir: None,
                launch_option_ids: Vec::new(),
                worktree: None,
            },
            "go",
            None,
        )
        .await
        .unwrap();
    let session_id = send.session_id.clone();
    assert_eq!(
        ix.live_state_for(&session_id).await.turn,
        TurnState::InFlight { send_id: None },
        "the session starts with its opening turn in flight"
    );
    (ix, events, session_id, stream)
}

#[tokio::test]
async fn death_mid_turn_settles_the_turn_and_every_pending_approval() {
    let (ix, mut events, session_id, stream) = codex_session_in_flight("thr_death_perm").await;

    // Two approvals arrive before any answer — the parallel fan-out a real
    // app-server produces — so both are pending when the process dies.
    for token in ["srv-1", "srv-2"] {
        stream
            .send(AgentEvent::PermissionRequested {
                request: AgentPermissionRequest {
                    request_id: token.to_owned(),
                    tool_name: "Bash".to_owned(),
                    input_json: json!({ "command": format!("cat {token}") }),
                    tool_use_id: None,
                },
            })
            .expect("the pump's stream is live");
    }
    let sid = session_id.clone();
    let pending = wait_for("both approval dialogs to be mirrored", || {
        let ix = &ix;
        let sid = sid.clone();
        async move {
            let queue = ix.live_state_for(&sid).await.pending_permissions;
            (queue.len() == 2).then_some(queue)
        }
    })
    .await;
    let request_ids: Vec<i64> = pending.iter().map(|p| p.request_id).collect();
    // Drop the events raised so far; what matters is what the *death* emits.
    while events.try_recv().is_ok() {}

    // The connection dies: the adapter surfaces the terminal fact.
    stream
        .send(AgentEvent::SessionEnded {
            reason: SessionEndReason::ProcessExited,
        })
        .expect("the pump's stream is live");

    // The browser converges from the event stream alone: one settle per dialog,
    // the turn ends as interrupted, and the session closes.
    let emitted = drain_until(&mut events, |event| {
        matches!(event, SessionEvent::SessionClosed { .. })
    })
    .await;
    let resolved: Vec<i64> = emitted
        .iter()
        .filter_map(|event| match event {
            SessionEvent::PermissionResolved { request_id, .. } => Some(*request_id),
            _ => None,
        })
        .collect();
    assert_eq!(
        resolved, request_ids,
        "every pending dialog is settled client-visibly: {emitted:?}"
    );
    assert!(
        !emitted
            .iter()
            .any(|event| matches!(event, SessionEvent::PermissionRequested { .. })),
        "no dialog is raised while clearing them all — a settle must not leave a \
         promoted head on screen: {emitted:?}"
    );
    assert!(
        emitted.iter().any(|event| matches!(
            event,
            SessionEvent::TurnInterrupted {
                session_id: sid,
                ..
            } if sid == &session_id
        )),
        "the stuck turn clears via the interrupted signal: {emitted:?}"
    );
    assert_eq!(
        emitted
            .iter()
            .filter(|event| matches!(event, SessionEvent::SessionClosed { .. }))
            .count(),
        1,
        "the session closes exactly once: {emitted:?}"
    );

    // The queryable truth a reconnecting browser refetches agrees: idle turn,
    // nothing pending, session closed.
    let live = ix.live_state_for(&session_id).await;
    assert_eq!(live.turn, TurnState::Idle, "the turn settled to idle");
    assert!(
        live.pending_permissions.is_empty(),
        "the sends envelope reports no pending dialog: {:?}",
        live.pending_permissions
    );
    assert!(
        !ix.is_session_open(&session_id).await,
        "the session reports itself closed after its process died"
    );

    // No row is left `pending`, and each says why it settled — a Deny with no
    // reason would be indistinguishable from a user's Deny.
    let rows = ix.store().inner.lock().unwrap().permissions.clone();
    assert_eq!(rows.len(), 2, "both requests were recorded: {rows:?}");
    for row in &rows {
        assert_eq!(
            row.status,
            PermissionStatus::Denied,
            "row {} was left pending: {row:?}",
            row.id
        );
        assert_eq!(
            row.decision_reason.as_deref(),
            Some("the agent session ended before this request could be answered"),
            "row {} records why it settled",
            row.id
        );
    }

    // A decision that arrives for a stranded request is a clean conflict (409),
    // not a write to a wire that no longer exists (which surfaced as a 500).
    let result = ix
        .decide_permission(request_ids[0], PermissionDecision::Allow)
        .await;
    assert!(
        matches!(result, Err(crate::error::Error::PermissionNotPending(id)) if id == request_ids[0]),
        "a decision for a stranded request is a conflict, got {result:?}"
    );
}

#[tokio::test]
async fn death_mid_turn_with_nothing_pending_still_interrupts_the_turn_and_closes() {
    // The plainest row of the matrix: no approval was ever raised, so the settle
    // is exactly the turn end plus the close — and it must not invent a
    // permission settle for a session that had none.
    let (ix, mut events, session_id, stream) = codex_session_in_flight("thr_death_plain").await;
    while events.try_recv().is_ok() {}

    stream
        .send(AgentEvent::SessionEnded {
            reason: SessionEndReason::ProcessExited,
        })
        .expect("the pump's stream is live");

    let emitted = drain_until(&mut events, |event| {
        matches!(event, SessionEvent::SessionClosed { .. })
    })
    .await;
    assert!(
        emitted
            .iter()
            .any(|event| matches!(event, SessionEvent::TurnInterrupted { .. })),
        "the in-flight turn still settles: {emitted:?}"
    );
    assert!(
        !emitted
            .iter()
            .any(|event| matches!(event, SessionEvent::PermissionResolved { .. })),
        "nothing was pending, so nothing is settled: {emitted:?}"
    );
    assert_eq!(ix.live_state_for(&session_id).await.turn, TurnState::Idle);
    assert!(!ix.is_session_open(&session_id).await);
}

#[tokio::test]
async fn a_send_after_a_death_resumes_the_session_and_runs_a_fresh_turn() {
    // Recovery stays explicit: Delta does not respawn the dead process, the next
    // Send does — over the existing resume path, which must still work against a
    // session the death closed.
    let factory = FakeAgentFactory::new("thr_death_resume", Some("turn_1"));
    let stream = factory.event_sender();
    let (ix, _events) = interactor_with_codex_factory_and_event_sink(factory.clone());
    let (first, _) = ix
        .enqueue_send(
            SendTarget::NewSession {
                provider: AgentProvider::Codex,
                workdir: None,
                launch_option_ids: Vec::new(),
                worktree: None,
            },
            "first message",
            None,
        )
        .await
        .unwrap();
    let session_id = first.session_id.clone();
    let main_thread = ix.store().main_thread_id(&session_id).await.unwrap();

    stream
        .send(AgentEvent::SessionEnded {
            reason: SessionEndReason::ProcessExited,
        })
        .expect("the pump's stream is live");
    let sid = session_id.clone();
    wait_for("the dead session to close", || {
        let ix = &ix;
        let sid = sid.clone();
        async move { (!ix.is_session_open(&sid).await).then_some(()) }
    })
    .await;

    // The next send reconnects the adapter against the persisted provider thread
    // and dispatches a fresh turn.
    let (second, _) = ix
        .enqueue_send(
            SendTarget::Thread {
                thread_id: main_thread,
                branch_from: None,
            },
            "second message",
            None,
        )
        .await
        .expect("a send to the settled session resumes it");
    assert_eq!(second.session_id, session_id, "the same session resumed");
    {
        let log = factory.log();
        let log = log.lock().unwrap();
        assert_eq!(
            log.resumes,
            vec!["thr_death_resume".to_owned()],
            "the resume reattached to the persisted provider thread"
        );
        assert_eq!(
            log.sends,
            vec!["first message".to_owned(), "second message".to_owned()],
            "the fresh turn dispatched over the reconnected adapter"
        );
    }
    assert!(
        ix.is_session_open(&session_id).await,
        "the resumed session is open again"
    );

    // The reconnected pump drives the fresh turn to completion, so the session is
    // usable — not merely re-bound.
    factory
        .event_sender()
        .send(AgentEvent::TurnCompleted {
            status: TurnStatus::Completed,
        })
        .expect("the reconnected pump's stream is live");
    let sid = session_id.clone();
    wait_for("the resumed turn to complete", || {
        let ix = &ix;
        let sid = sid.clone();
        async move { matches!(ix.live_state_for(&sid).await.turn, TurnState::Idle).then_some(()) }
    })
    .await;
}

#[tokio::test]
async fn death_while_idle_closes_the_session_without_a_turn_signal() {
    let (ix, mut events, session_id, stream) = codex_session_in_flight("thr_death_idle").await;

    // The turn completes cleanly first, so the session is idle when the process
    // dies.
    stream
        .send(AgentEvent::TurnCompleted {
            status: TurnStatus::Completed,
        })
        .expect("the pump's stream is live");
    let sid = session_id.clone();
    wait_for("the turn to settle to idle", || {
        let ix = &ix;
        let sid = sid.clone();
        async move { matches!(ix.live_state_for(&sid).await.turn, TurnState::Idle).then_some(()) }
    })
    .await;
    while events.try_recv().is_ok() {}

    stream
        .send(AgentEvent::SessionEnded {
            reason: SessionEndReason::ProcessExited,
        })
        .expect("the pump's stream is live");

    let emitted = drain_until(&mut events, |event| {
        matches!(event, SessionEvent::SessionClosed { .. })
    })
    .await;
    assert!(
        !emitted.iter().any(|event| matches!(
            event,
            SessionEvent::TurnInterrupted { .. } | SessionEvent::TurnCompleted { .. }
        )),
        "an idle death invents no turn end — there was no turn to interrupt: {emitted:?}"
    );
    assert!(
        !ix.is_session_open(&session_id).await,
        "an idle session whose process died still reports itself closed"
    );
}

#[tokio::test]
async fn an_orderly_close_settles_once_and_reports_no_failure() {
    // The close path is unchanged: it already tears the session down
    // synchronously, and the adapter's own `SessionEnded { Closed }` — which
    // flows through the same pump — must not settle a second time.
    let (ix, mut events, session_id, stream) = codex_session_in_flight("thr_close").await;

    ix.close_session(&session_id)
        .await
        .expect("the Codex session closes");
    assert!(
        !ix.is_session_open(&session_id).await,
        "close_session closed the session"
    );

    // The adapter emits its orderly end on the stream (as the real one does from
    // `close`). The pump must treat it as already handled.
    stream
        .send(AgentEvent::SessionEnded {
            reason: SessionEndReason::Closed,
        })
        .expect("the pump's stream is live");
    // A settle would emit on the seam; give the pump a moment to prove it does
    // not. Nothing else emits asynchronously on this path, so an empty seam is
    // the assertion.
    tokio::time::sleep(Duration::from_millis(50)).await;
    let leaked: Vec<SessionEvent> = std::iter::from_fn(|| events.try_recv().ok()).collect();
    assert!(
        !leaked
            .iter()
            .any(|event| matches!(event, SessionEvent::SessionClosed { .. })),
        "an orderly close is announced by the close path itself, never twice: {leaked:?}"
    );
    assert!(
        !leaked
            .iter()
            .any(|event| matches!(event, SessionEvent::TurnInterrupted { .. })),
        "an orderly close produces no failure signal: {leaked:?}"
    );
}
