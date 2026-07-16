//! The provider-neutral adapter contract suite, run against
//! [`CodexAppServerAdapter`] driven by the real `fake-codex` app-server binary.
//!
//! Two layers run here:
//!
//! 1. The **shared** cases from [`agent_contract`] — the mechanical operations
//!    every adapter must satisfy — run against the Codex adapter unchanged,
//!    proving the neutral contract is provider-independent.
//! 2. **Codex-specific** cases drive the scripted app-server through a full turn:
//!    the structured `turn/*` / `item/*` notifications translate into
//!    `TurnStarted` / `AssistantMessage` / `ToolStarted` / `ToolCompleted` /
//!    `TurnCompleted`, an `*/requestApproval` server request becomes a
//!    `PermissionRequested` the adapter answers (allow → `accept`, deny →
//!    `decline`), `turn/interrupt` ends the turn, and — the invariant that
//!    matters most for an app-server with no interactive fallback — an unmodeled
//!    server request surfaces as `UnsupportedInteraction` without the turn
//!    hanging.
//!
//! Correctness here is "against the fake": the wire shapes are the inferred
//! contract shared by `codex-agent`'s `wire`/`translate` modules and these
//! scenarios. Real-`codex` verification is a later phase.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use agent_contract::launch_request;
use codex_agent::{AppServerConnection, CodexAppServerAdapter, CodexLaunchConfig};
use delta_usecase::{
    AgentAdapter, AgentEvent, AgentEventStream, PermissionDecision, ResumeRequest, SendRequest,
    TurnStatus,
};
use serde_json::json;
use tokio::time::timeout;

/// A short bound so a wiring bug fails fast instead of hanging the suite.
const TIMEOUT: Duration = Duration::from_secs(10);

// --- Fixture ----------------------------------------------------------------

/// A scenario file written to a unique temp dir, removed on drop.
struct ScenarioGuard {
    dir: PathBuf,
    path: PathBuf,
}

impl ScenarioGuard {
    fn write(scenario: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("fake-codex-adapter-{}-{}", std::process::id(), n));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("scenario.json");
        std::fs::write(&path, scenario).unwrap();
        Self { dir, path }
    }
}

impl Drop for ScenarioGuard {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).ok();
    }
}

/// Spawn the fake with an explicit scenario and build an initialised adapter
/// over the connection. The guard must be kept alive for the fake's lifetime
/// (it owns the scenario file on disk).
async fn adapter_with(scenario: &str) -> (CodexAppServerAdapter, ScenarioGuard) {
    let guard = ScenarioGuard::write(scenario);
    let config = CodexLaunchConfig {
        codex_bin: env!("CARGO_BIN_EXE_fake-codex").to_owned(),
        args: vec![],
        env: vec![(
            "FAKE_CODEX_SCENARIO".to_owned(),
            guard.path.to_string_lossy().into_owned(),
        )],
    };
    let conn = Arc::new(AppServerConnection::spawn(&config).expect("spawn fake-codex"));
    conn.initialize(json!({ "clientInfo": { "name": "delta" } }))
        .await
        .expect("initialize");
    (CodexAppServerAdapter::new(conn), guard)
}

/// Spawn the fake with its built-in default scenario (one short assistant-message
/// turn), used by the mechanical shared cases.
async fn default_adapter() -> CodexAppServerAdapter {
    let config = CodexLaunchConfig {
        codex_bin: env!("CARGO_BIN_EXE_fake-codex").to_owned(),
        args: vec![],
        env: vec![],
    };
    let conn = Arc::new(AppServerConnection::spawn(&config).expect("spawn fake-codex"));
    conn.initialize(json!({})).await.expect("initialize");
    CodexAppServerAdapter::new(conn)
}

/// Receive events until `stop` returns true for one (inclusive), or the per-event
/// timeout fires (a hang — which is itself a contract failure). The stream
/// closing early also stops the collection.
async fn collect_until<F>(stream: &mut AgentEventStream, stop: F) -> Vec<AgentEvent>
where
    F: Fn(&AgentEvent) -> bool,
{
    let mut events = Vec::new();
    loop {
        match timeout(TIMEOUT, stream.recv()).await {
            Ok(Some(event)) => {
                let done = stop(&event);
                events.push(event);
                if done {
                    break;
                }
            }
            Ok(None) => break,
            Err(_) => panic!("timed out waiting for an event; collected so far: {events:?}"),
        }
    }
    events
}

fn is_turn_completed(event: &AgentEvent) -> bool {
    matches!(event, AgentEvent::TurnCompleted { .. })
}

/// A turn scenario that emits a full assistant-message turn, with an optional
/// extra emission spliced in before completion.
fn turn_scenario(extra_emit: &str) -> String {
    format!(
        r#"{{
            "thread_id": "thr_contract",
            "turn": {{
                "turn_id": "turn_contract",
                "emit": [
                    {{ "type": "turn_started" }},
                    {{ "type": "item_started", "item": {{ "id": "m1", "itemType": "agent_message" }} }},
                    {extra_emit}
                    {{ "type": "item_completed", "item": {{ "id": "m1", "itemType": "agent_message", "text": "scripted reply" }} }},
                    {{ "type": "turn_completed", "status": "completed" }}
                ]
            }}
        }}"#
    )
}

// --- Shared mechanical cases (identical bodies for both adapters) ------------

#[tokio::test]
async fn launch_returns_provider_session_id() {
    agent_contract::case_launch_returns_provider_session_id(&default_adapter().await).await;
}

#[tokio::test]
async fn send_emits_user_prompt_accepted() {
    agent_contract::case_send_emits_user_prompt_accepted(&default_adapter().await).await;
}

#[tokio::test]
async fn context_injection_does_not_pollute_visible_prompt() {
    agent_contract::case_context_injection_does_not_pollute_visible_prompt(
        &default_adapter().await,
    )
    .await;
}

#[tokio::test]
async fn interrupt_is_accepted_when_supported() {
    agent_contract::case_interrupt_is_accepted_when_supported(&default_adapter().await).await;
}

#[tokio::test]
async fn close_ends_the_session() {
    agent_contract::case_close_ends_the_session(&default_adapter().await).await;
}

// --- Codex-specific cases (scripted app-server turns) ------------------------

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

/// `send_emits_user_prompt_and_turn_started`: a send emits `UserPromptAccepted`
/// and the turn's `turn/started` notification projects `TurnStarted`.
#[tokio::test]
async fn send_emits_user_prompt_and_turn_started() {
    let (adapter, _guard) = adapter_with(&turn_scenario("")).await;
    let handle = adapter.launch(launch_request()).await.expect("launch");
    let mut stream = adapter.events(&handle);
    adapter
        .send(
            &handle,
            SendRequest {
                text: "hello codex".to_owned(),
            },
        )
        .await
        .expect("send");
    let events = collect_until(&mut stream, is_turn_completed).await;
    assert!(
        events.iter().any(
            |e| matches!(e, AgentEvent::UserPromptAccepted { text, .. } if text == "hello codex")
        ),
        "expected UserPromptAccepted, got {events:?}"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, AgentEvent::TurnStarted { .. })),
        "expected TurnStarted, got {events:?}"
    );
}

/// `turn_completion_is_emitted_once`: exactly one `TurnCompleted(Completed)` per
/// turn, and the completed assistant message replays as `AssistantMessage`.
#[tokio::test]
async fn turn_completion_is_emitted_once_and_carries_the_assistant_message() {
    let (adapter, _guard) = adapter_with(&turn_scenario("")).await;
    let handle = adapter.launch(launch_request()).await.expect("launch");
    let mut stream = adapter.events(&handle);
    adapter
        .send(
            &handle,
            SendRequest {
                text: "go".to_owned(),
            },
        )
        .await
        .expect("send");
    let events = collect_until(&mut stream, is_turn_completed).await;
    let completions = events
        .iter()
        .filter(|e| {
            matches!(
                e,
                AgentEvent::TurnCompleted {
                    status: TurnStatus::Completed
                }
            )
        })
        .count();
    assert_eq!(
        completions, 1,
        "exactly one TurnCompleted per turn: {events:?}"
    );
    assert!(
        events.iter().any(
            |e| matches!(e, AgentEvent::AssistantMessage { text, .. } if text == "scripted reply")
        ),
        "expected the assistant message, got {events:?}"
    );
}

/// Tool items translate to `ToolStarted` / `ToolCompleted`.
#[tokio::test]
async fn tool_items_translate_to_tool_events() {
    let scenario = r#"{
        "thread_id": "thr_tool",
        "turn": {
            "turn_id": "turn_tool",
            "emit": [
                { "type": "turn_started" },
                { "type": "item_started", "item": { "id": "t1", "itemType": "command_execution", "input": { "command": "ls" } } },
                { "type": "item_completed", "item": { "id": "t1", "itemType": "command_execution", "output": { "exitCode": 0 } } },
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
                text: "run it".to_owned(),
            },
        )
        .await
        .expect("send");
    let events = collect_until(&mut stream, is_turn_completed).await;
    assert!(
        events.iter().any(|e| matches!(
            e,
            AgentEvent::ToolStarted { name, .. } if name == "command_execution"
        )),
        "expected ToolStarted, got {events:?}"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, AgentEvent::ToolCompleted { .. })),
        "expected ToolCompleted, got {events:?}"
    );
}

/// `permission_request_can_be_allowed`: an approval request surfaces as
/// `PermissionRequested`; answering allow emits `PermissionResolved(Allow)`.
#[tokio::test]
async fn permission_request_can_be_allowed() {
    permission_case(PermissionDecision::Allow).await;
}

/// `permission_request_can_be_denied`: the same path, answered deny.
#[tokio::test]
async fn permission_request_can_be_denied() {
    permission_case(PermissionDecision::Deny).await;
}

async fn permission_case(decision: PermissionDecision) {
    let extra =
        r#"{ "type": "request_approval", "params": { "itemId": "m1", "toolName": "Bash" } },"#;
    let (adapter, _guard) = adapter_with(&turn_scenario(extra)).await;
    let handle = adapter.launch(launch_request()).await.expect("launch");
    let mut stream = adapter.events(&handle);
    adapter
        .send(
            &handle,
            SendRequest {
                text: "do it".to_owned(),
            },
        )
        .await
        .expect("send");

    // Collect up to and including the PermissionRequested, then answer it.
    let before = collect_until(&mut stream, |e| {
        matches!(e, AgentEvent::PermissionRequested { .. })
    })
    .await;
    let request_id = before
        .iter()
        .find_map(|e| match e {
            AgentEvent::PermissionRequested { request } => Some(request.request_id.clone()),
            _ => None,
        })
        .expect("a PermissionRequested to answer");
    assert!(
        before.iter().any(|e| matches!(
            e,
            AgentEvent::PermissionRequested { request } if request.tool_name == "Bash"
        )),
        "the approval carries its tool name: {before:?}"
    );

    adapter
        .resolve_permission(&handle, &request_id, decision)
        .await
        .expect("resolve_permission");

    let after = collect_until(&mut stream, |e| {
        matches!(e, AgentEvent::PermissionResolved { .. })
    })
    .await;
    assert!(
        after.iter().any(|e| matches!(
            e,
            AgentEvent::PermissionResolved { request_id: rid, decision: d }
                if *rid == request_id && *d == decision
        )),
        "expected PermissionResolved({decision:?}) for {request_id}, got {after:?}"
    );
}

/// `interrupt_ends_turn`: interrupting drives `turn/interrupt`, whose
/// `turn/completed{interrupted}` projects `TurnCompleted(Interrupted)`.
#[tokio::test]
async fn interrupt_ends_turn() {
    // A turn that emits nothing on its own, so the only completion is the one
    // the interrupt produces.
    let scenario = r#"{
        "thread_id": "thr_interrupt",
        "turn": { "turn_id": "turn_interrupt", "emit": [ { "type": "turn_started" } ] }
    }"#;
    let (adapter, _guard) = adapter_with(scenario).await;
    let handle = adapter.launch(launch_request()).await.expect("launch");
    let mut stream = adapter.events(&handle);
    adapter
        .send(
            &handle,
            SendRequest {
                text: "long task".to_owned(),
            },
        )
        .await
        .expect("send");
    adapter.interrupt(&handle).await.expect("interrupt");
    let events = collect_until(&mut stream, is_turn_completed).await;
    assert!(
        events.iter().any(|e| matches!(
            e,
            AgentEvent::TurnCompleted {
                status: TurnStatus::Interrupted
            }
        )),
        "expected TurnCompleted(Interrupted), got {events:?}"
    );
}

/// `no_server_request_silently_hangs`: an unmodeled server → client request
/// surfaces as `UnsupportedInteraction`, and the turn still completes (the
/// adapter answered the request rather than blocking on it).
#[tokio::test]
async fn no_server_request_silently_hangs() {
    // A server request whose method is NOT an approval: the adapter does not
    // model it, so it must reject it and surface it — never hang.
    let extra = r#"{ "type": "request_approval", "method": "session/requestUserInput", "params": { "prompt": "pick one" } },"#;
    let (adapter, _guard) = adapter_with(&turn_scenario(extra)).await;
    let handle = adapter.launch(launch_request()).await.expect("launch");
    let mut stream = adapter.events(&handle);
    adapter
        .send(
            &handle,
            SendRequest {
                text: "trigger the unknown request".to_owned(),
            },
        )
        .await
        .expect("send");
    // If the adapter blocked on the unmodeled request, the turn would never
    // complete and this collect would time out (a failure).
    let events = collect_until(&mut stream, is_turn_completed).await;
    assert!(
        events.iter().any(|e| matches!(
            e,
            AgentEvent::UnsupportedInteraction { method, .. } if method == "session/requestUserInput"
        )),
        "an unmodeled server request must surface as UnsupportedInteraction, got {events:?}"
    );
    assert!(
        events.iter().any(is_turn_completed),
        "the turn must still complete (no silent hang), got {events:?}"
    );
}
