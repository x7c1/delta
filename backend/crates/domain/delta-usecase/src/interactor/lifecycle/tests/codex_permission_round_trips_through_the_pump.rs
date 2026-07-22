//! Permission correlation for a terminal-less (Codex) session, end to end
//! through the interactor.
//!
//! The adapter speaks its own opaque approval token; the domain speaks only the
//! Delta `i64` permission-row id. This test drives the full round-trip over the
//! session's event pump and the browser-decision path:
//!
//! 1. The adapter surfaces a `PermissionRequested` carrying its provider token.
//!    The pump allocates an `i64` row, correlates it with the token, indexes it
//!    for decision routing, and raises the queryable mirror **under the `i64`
//!    id** (so [`reduce_permission_event`] never sees a non-`i64` id — the
//!    invariant it panics on).
//! 2. `decide_permission(i64)` (the REST path's entry) routes to the owning
//!    session, translates the `i64` back to the provider token, and answers the
//!    adapter over the trait with the correct decision.
//! 3. The adapter emits `PermissionResolved`; the pump translates the token back
//!    to the `i64`, clears the mirror, and drops the correlation.
//!
//! [`reduce_permission_event`]: crate::interactor::agent_permission::reduce_permission_event

use std::time::Duration;

use delta_model::AgentProvider;
use serde_json::json;

use crate::agent::{AgentEvent, AgentPermissionRequest};
use crate::interactor::testing::*;
use crate::interactor::PermissionDecision;
use crate::SendTarget;

/// Poll `f` until it returns `Some`, or panic after a short deadline. The event
/// pump runs on a background task, so its effect on the runtime mirror lands
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
async fn codex_permission_round_trips_through_the_pump() {
    let factory = FakeAgentFactory::new("thr_perm", Some("turn_perm"));
    let events = factory.event_sender();
    let log = factory.log();
    let ix = interactor_with_codex_factory(factory);

    // Stand up a Codex session (its event pump starts draining the adapter).
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

    // The adapter surfaces an approval carrying its opaque provider token.
    events
        .send(AgentEvent::PermissionRequested {
            request: AgentPermissionRequest {
                request_id: "srv-1".to_owned(),
                tool_name: "Bash".to_owned(),
                input_json: json!({ "command": "ls" }),
                tool_use_id: None,
            },
        })
        .expect("the pump's stream is live");

    // The pump allocated an i64 row and raised the mirror under it — proving
    // `reduce_permission_event` saw an i64 (it panics otherwise) and the notice
    // shows the row id, not the provider token.
    let sid = session_id.clone();
    let pending = wait_for("the permission mirror to rise", || {
        let ix = &ix;
        let sid = sid.clone();
        async move { ix.live_state_for(&sid).await.pending_permission }
    })
    .await;
    assert!(
        pending.request_id > 0,
        "the mirror carries a Delta row id, not the provider token"
    );
    assert_eq!(pending.tool_name, "Bash");
    assert_eq!(pending.tool_input_json, r#"{"command":"ls"}"#);
    let row_id = pending.request_id;

    // The browser decides by the i64 row id. Routing translates it back to the
    // provider token and answers the adapter over the trait.
    let broadcast = ix
        .decide_permission(row_id, PermissionDecision::Allow)
        .await
        .expect("the decision routes to the Codex session");
    assert!(
        broadcast.is_empty(),
        "a Codex decision settles asynchronously through the pump, not synchronously"
    );

    // The adapter received exactly the provider token and decision it was
    // correlated with — the i64 ↔ token translation is correct.
    assert_eq!(
        *log.lock().unwrap().resolves,
        [("srv-1".to_owned(), PermissionDecision::Allow)],
        "resolve_permission was called with the provider token and the decision"
    );

    // The adapter's emitted `PermissionResolved` flows back through the pump,
    // which clears the mirror (token → i64 again, no panic) and drops the
    // correlation.
    let sid = session_id.clone();
    wait_for("the permission mirror to clear", || {
        let ix = &ix;
        let sid = sid.clone();
        async move {
            ix.live_state_for(&sid)
                .await
                .pending_permission
                .is_none()
                .then_some(())
        }
    })
    .await;
}

#[tokio::test]
async fn deciding_an_unknown_codex_permission_is_a_conflict() {
    // A decision for a request that was never surfaced has no index entry, so it
    // is a clean conflict rather than a mis-route.
    let factory = FakeAgentFactory::new("thr_none", Some("turn_none"));
    let ix = interactor_with_codex_factory(factory);
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
    let _ = send;

    let result = ix.decide_permission(9999, PermissionDecision::Deny).await;
    assert!(
        matches!(result, Err(crate::error::Error::PermissionNotPending(9999))),
        "an unknown permission id is PermissionNotPending, got {result:?}"
    );
}
