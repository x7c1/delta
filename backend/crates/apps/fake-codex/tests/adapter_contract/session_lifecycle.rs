//! The session's lifecycle over the adapter: adopting the server-minted thread
//! id, resuming an existing thread, a mid-turn death, and an orderly close.

use agent_contract::launch_request;
use delta_usecase::{AgentAdapter, AgentEvent, ResumeRequest, SendRequest};
use tokio::time::timeout;

use crate::support::{
    adapter_with, collect_until, default_adapter, is_turn_completed, turn_scenario, TIMEOUT,
};

/// `launch_returns_provider_session_id` (Codex specifics): the id is the
/// server-minted thread id, not Delta's session id.
#[tokio::test]
async fn launch_adopts_the_server_minted_thread_id() {
    let (adapter, _guard) = adapter_with(&turn_scenario("")).await;
    let handle = adapter.launch(launch_request()).await.expect("launch");
    assert_eq!(
        handle.provider_session_id, "thr_contract",
        "the provider session id is the server's thread id"
    );
    assert_ne!(
        handle.provider_session_id,
        launch_request().session_id,
        "Codex does not pin Delta's own session id"
    );
}

/// `resume_loads_existing_session`: resuming maps to `thread/resume` and adopts
/// the resumed thread id, opening with a `SessionStarted`.
#[tokio::test]
async fn resume_loads_existing_session() {
    let adapter = default_adapter().await;
    let handle = adapter
        .resume(ResumeRequest {
            session_id: "delta-sid".to_owned(),
            provider_session_id: "thr_existing".to_owned(),
            workdir: "/tmp/workdir".to_owned(),
            worktree_repo_root: None,
        })
        .await
        .expect("resume");
    assert_eq!(handle.provider_session_id, "thr_existing");
    let mut stream = adapter.events(&handle);
    match timeout(TIMEOUT, stream.recv()).await.unwrap() {
        Some(AgentEvent::SessionStarted {
            provider_session_id,
        }) => assert_eq!(provider_session_id, "thr_existing"),
        other => panic!("expected SessionStarted, got {other:?}"),
    }
}

// --- Session death: the app-server process going away mid-turn ---------------

/// A dying app-server surfaces a **terminal event**, not silence.
///
/// The field failure: the process was killed while a turn was in flight with an
/// approval on screen. The adapter's event stream simply went quiet — which is
/// indistinguishable from a slow model — so the turn stayed in flight forever and
/// the dialog could never be answered. Here the fake plays the same sequence (an
/// approval, then it dies with the approval unanswered and the turn unfinished)
/// and the stream must report `SessionEnded { ProcessExited }`.
#[tokio::test]
async fn a_dying_app_server_ends_the_session_as_process_exited() {
    let scenario = r#"{
        "thread_id": "thr_death",
        "turn": {
            "turn_id": "turn_death",
            "emit": [
                { "type": "turn_started" },
                { "type": "request_approval", "params": { "itemId": "exec_1", "command": "date", "cwd": "/tmp" } },
                { "type": "exit" },
                { "type": "turn_completed", "status": "completed" }
            ]
        }
    }"#;
    let (adapter, _guard) = adapter_with(scenario).await;
    let handle = adapter.launch(launch_request()).await.expect("launch");
    let mut stream = adapter.events(&handle);
    adapter
        .send(
            &handle,
            SendRequest {
                text: "run a command".to_owned(),
            },
        )
        .await
        .expect("send");

    let events = collect_until(&mut stream, |e| {
        matches!(e, AgentEvent::SessionEnded { .. })
    })
    .await;
    assert!(
        events.iter().any(|e| matches!(
            e,
            AgentEvent::SessionEnded {
                reason: delta_usecase::SessionEndReason::ProcessExited
            }
        )),
        "a process that died mid-turn must end the session as an exit, got {events:?}"
    );
    // The approval the server managed to write is delivered *before* the death,
    // so the core settles it in the order it happened rather than discovering it
    // after the session is already gone.
    let requested = events
        .iter()
        .position(|e| matches!(e, AgentEvent::PermissionRequested { .. }))
        .expect("the approval that was raised before the death is delivered");
    let ended = events
        .iter()
        .position(|e| matches!(e, AgentEvent::SessionEnded { .. }))
        .expect("the death is on the stream");
    assert!(
        requested < ended,
        "the last frames precede the death on the stream, got {events:?}"
    );
    // The turn never completed on the wire: the death is the only end.
    assert!(
        !events.iter().any(is_turn_completed),
        "the killed turn has no completion — that is the whole problem, got {events:?}"
    );
}

/// An orderly `close` never reports the failure variant, even though the
/// connection behind it is torn down right afterwards: the reason is what the
/// core branches on, so a normal close must stay `Closed`.
#[tokio::test]
async fn an_orderly_close_never_reports_a_process_exit() {
    let (adapter, _guard) = adapter_with(&turn_scenario("")).await;
    let handle = adapter.launch(launch_request()).await.expect("launch");
    let mut stream = adapter.events(&handle);
    adapter.close(&handle).await.expect("close");
    // Dropping the adapter kills the fake (its connection owns the child), so a
    // spurious death signal would surface here if the close path produced one.
    drop(adapter);

    let mut ends = Vec::new();
    while let Ok(Some(event)) = timeout(TIMEOUT, stream.recv()).await {
        if let AgentEvent::SessionEnded { reason } = event {
            ends.push(reason);
        }
    }
    assert_eq!(
        ends,
        vec![delta_usecase::SessionEndReason::Closed],
        "a closed session ends exactly once, as a close"
    );
}
