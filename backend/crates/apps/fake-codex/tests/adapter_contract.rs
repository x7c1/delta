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
//!    `TurnCompleted`, the real `item/commandExecution/requestApproval` and
//!    `item/fileChange/requestApproval` server requests become a
//!    `PermissionRequested` the adapter answers (allow → `accept`, deny →
//!    `decline`), `turn/interrupt` ends the turn, and — the invariant that
//!    matters most for an app-server with no interactive fallback — a server
//!    request Delta does not model (including `item/permissions/requestApproval`,
//!    whose response is a permission profile rather than a decision) surfaces as
//!    `UnsupportedInteraction` without the turn hanging.
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
    AgentAdapter, AgentEvent, AgentEventStream, AgentFileChange, AgentFileChangeKind,
    PermissionDecision, ResumeRequest, SendRequest, SessionEndReason, TurnStatus,
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
    conn.initialize(json!({ "clientInfo": { "name": "delta", "version": "0" } }))
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
    conn.initialize(json!({ "clientInfo": { "name": "delta", "version": "0" } }))
        .await
        .expect("initialize");
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
                    {{ "type": "item_started", "item": {{ "id": "m1", "type": "agentMessage" }} }},
                    {{ "type": "agent_message_delta", "item_id": "m1", "delta": "scripted reply" }},
                    {extra_emit}
                    {{ "type": "item_completed", "item": {{ "id": "m1", "type": "agentMessage", "text": "scripted reply" }} }},
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
                { "type": "item_started", "item": { "id": "t1", "type": "commandExecution", "command": "ls", "cwd": "/tmp", "status": "inProgress", "commandActions": [] } },
                { "type": "item_completed", "item": { "id": "t1", "type": "commandExecution", "command": "ls", "cwd": "/tmp", "status": "completed", "commandActions": [], "aggregatedOutput": "a\nb", "exitCode": 0 } },
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

/// A command-execution approval surfaces as `PermissionRequested` (its `command`
/// naming the tool); answering allow emits `PermissionResolved(Allow)`.
#[tokio::test]
async fn command_execution_permission_can_be_allowed() {
    permission_case(
        PermissionDecision::Allow,
        command_execution_approval(),
        "date",
    )
    .await;
}

/// The same command-execution path, answered deny.
#[tokio::test]
async fn command_execution_permission_can_be_denied() {
    permission_case(
        PermissionDecision::Deny,
        command_execution_approval(),
        "date",
    )
    .await;
}

/// A file-change approval surfaces as `PermissionRequested` (named by its kind,
/// as its params carry no command); answering allow emits
/// `PermissionResolved(Allow)`.
#[tokio::test]
async fn file_change_permission_can_be_allowed() {
    permission_case(
        PermissionDecision::Allow,
        file_change_approval(),
        "file_change",
    )
    .await;
}

/// The same file-change path, answered deny.
#[tokio::test]
async fn file_change_permission_can_be_denied() {
    permission_case(
        PermissionDecision::Deny,
        file_change_approval(),
        "file_change",
    )
    .await;
}

/// A scripted command-execution approval step, with the real method + params.
fn command_execution_approval() -> &'static str {
    r#"{ "type": "request_approval", "method": "item/commandExecution/requestApproval",
         "params": { "itemId": "m1", "command": "date", "cwd": "/tmp" } },"#
}

/// A scripted file-change approval step, with the real method + params.
fn file_change_approval() -> &'static str {
    r#"{ "type": "request_approval", "method": "item/fileChange/requestApproval",
         "params": { "itemId": "m1", "grantRoot": "/repo", "reason": "write access" } },"#
}

async fn permission_case(decision: PermissionDecision, extra: &str, expected_tool: &str) {
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
            AgentEvent::PermissionRequested { request } if request.tool_name == expected_tool
        )),
        "the approval carries its tool name `{expected_tool}`: {before:?}"
    );

    // Answer through `&dyn AgentAdapter` (not the concrete type) so the test
    // proves the decision seam is reachable over the trait object the core holds
    // — `resolve_permission` is a trait method, not an inherent one.
    let adapter_dyn: &dyn AgentAdapter = &adapter;
    adapter_dyn
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

/// `no_server_request_silently_hangs` (permissions approval): the permissions
/// approval is a real `*/requestApproval` method, but its response is a
/// permission profile Delta cannot synthesise — so it must NOT be treated as an
/// approval. It surfaces as `UnsupportedInteraction` and the turn still
/// completes (the adapter answered it rather than blocking on it).
#[tokio::test]
async fn permissions_approval_is_unsupported_and_never_hangs() {
    no_server_request_silently_hangs_for(
        r#"{ "type": "request_approval", "method": "item/permissions/requestApproval",
             "params": { "itemId": "m1", "cwd": "/tmp", "permissions": {} } },"#,
        "item/permissions/requestApproval",
    )
    .await;
}

/// `no_server_request_silently_hangs` (unknown method): a server → client request
/// Delta does not model at all surfaces as `UnsupportedInteraction` without the
/// turn hanging.
#[tokio::test]
async fn unknown_server_request_is_unsupported_and_never_hangs() {
    no_server_request_silently_hangs_for(
        r#"{ "type": "request_approval", "method": "item/tool/requestUserInput", "params": { "questions": [] } },"#,
        "item/tool/requestUserInput",
    )
    .await;
}

/// Drive a turn that emits a server → client request the adapter does not model,
/// asserting it surfaces as `UnsupportedInteraction` for `method` and the turn
/// still completes (the adapter answered the request rather than blocking on it).
async fn no_server_request_silently_hangs_for(extra: &str, method: &str) {
    let (adapter, _guard) = adapter_with(&turn_scenario(extra)).await;
    let handle = adapter.launch(launch_request()).await.expect("launch");
    let mut stream = adapter.events(&handle);
    adapter
        .send(
            &handle,
            SendRequest {
                text: "trigger the unmodeled request".to_owned(),
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
            AgentEvent::UnsupportedInteraction { method: m, .. } if m == method
        )),
        "`{method}` must surface as UnsupportedInteraction, got {events:?}"
    );
    assert!(
        events.iter().any(is_turn_completed),
        "the turn must still complete (no silent hang), got {events:?}"
    );
}

// --- Usage: token accounting and account rate limits -------------------------

/// A turn that reports its token usage: the thread-scoped
/// `thread/tokenUsage/updated` frame must translate into a neutral usage event
/// carrying the counts AND a percentage computed from `modelContextWindow` —
/// the frame no longer dies in the translator's catch-all.
///
/// The fixture keeps a real session's proportions, where the running `total`
/// has long since passed the window (250% here) while the last call — the
/// conversation actually occupying it — is a quarter of it. That is what makes
/// the percentage below an assertion about reading `last`, not just arithmetic.
#[tokio::test]
async fn a_turns_token_usage_surfaces_with_a_percentage_of_the_context_window() {
    let (adapter, _guard) = adapter_with(&turn_scenario(
        r#"{ "type": "notification", "method": "thread/tokenUsage/updated",
             "params": { "turnId": "turn_contract", "tokenUsage": {
                 "total": { "totalTokens": 500000, "inputTokens": 480000, "cachedInputTokens": 400000,
                            "outputTokens": 20000, "reasoningOutputTokens": 5000 },
                 "last": { "totalTokens": 50000, "inputTokens": 48000, "cachedInputTokens": 40000,
                           "outputTokens": 2000, "reasoningOutputTokens": 500 },
                 "modelContextWindow": 200000 } } },"#,
    ))
    .await;
    let handle = adapter.launch(launch_request()).await.expect("launch");
    let mut stream = adapter.events(&handle);
    adapter
        .send(
            &handle,
            SendRequest {
                text: "spend some tokens".to_owned(),
            },
        )
        .await
        .expect("send");

    let events = collect_until(&mut stream, is_turn_completed).await;
    let usage = events
        .iter()
        .find_map(|event| match event {
            AgentEvent::TokenUsageUpdated { usage } => Some(usage.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no token usage surfaced; got {events:?}"));
    assert_eq!(
        usage.context_used_percentage,
        Some(25.0),
        "the last call's 50k of a 200k window is 25%, computed at the Codex edge"
    );
    assert_eq!(usage.context_window_size, Some(200_000));
    assert_eq!(usage.context_current_usage, Some(50_000));
    // The one cumulative reading, so this one comes from `total`.
    assert_eq!(usage.total_input_tokens, Some(480_000));
}

/// The same turn without a `modelContextWindow`: the counts still surface, and
/// the percentage is omitted rather than fabricated (which is what makes the
/// browser hide the bar instead of drawing a meaningless one).
#[tokio::test]
async fn token_usage_without_a_context_window_surfaces_no_percentage() {
    let (adapter, _guard) = adapter_with(&turn_scenario(
        r#"{ "type": "notification", "method": "thread/tokenUsage/updated",
             "params": { "turnId": "turn_contract", "tokenUsage": {
                 "total": { "totalTokens": 500000, "inputTokens": 480000, "cachedInputTokens": 400000,
                            "outputTokens": 20000, "reasoningOutputTokens": 5000 },
                 "last": { "totalTokens": 50000, "inputTokens": 48000, "cachedInputTokens": 40000,
                           "outputTokens": 2000, "reasoningOutputTokens": 500 },
                 "modelContextWindow": null } } },"#,
    ))
    .await;
    let handle = adapter.launch(launch_request()).await.expect("launch");
    let mut stream = adapter.events(&handle);
    adapter
        .send(
            &handle,
            SendRequest {
                text: "spend some tokens".to_owned(),
            },
        )
        .await
        .expect("send");

    let events = collect_until(&mut stream, is_turn_completed).await;
    let usage = events
        .iter()
        .find_map(|event| match event {
            AgentEvent::TokenUsageUpdated { usage } => Some(usage.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no token usage surfaced; got {events:?}"));
    assert_eq!(usage.context_used_percentage, None);
    assert_eq!(usage.context_current_usage, Some(50_000));
}

/// An `account/rateLimits/updated` emitted with **no `threadId`** — the way the
/// real server emits it — reaches the session's event stream anyway, through the
/// adapter's connection-level drain of the unrouted channel.
///
/// The `account_notification` scenario step is what makes this a real test: an
/// ordinary `notification` step would have `threadId` stamped in and would
/// exercise the per-thread demux instead, passing while the production path (no
/// thread id at all) stayed broken.
#[tokio::test]
async fn account_rate_limits_reach_the_session_without_a_thread_id() {
    let (adapter, _guard) = adapter_with(&turn_scenario(
        r#"{ "type": "account_notification", "method": "account/rateLimits/updated",
             "params": { "rateLimits": {
                 "primary": { "usedPercent": 21, "resetsAt": 1700000000, "windowDurationMins": 300 },
                 "secondary": { "usedPercent": 4, "resetsAt": 1700500000, "windowDurationMins": 10080 },
                 "planType": "pro" } } },"#,
    ))
    .await;
    let handle = adapter.launch(launch_request()).await.expect("launch");
    let mut stream = adapter.events(&handle);
    adapter
        .send(
            &handle,
            SendRequest {
                text: "trigger the account update".to_owned(),
            },
        )
        .await
        .expect("send");

    // The account frame arrives on a different task from the turn's own frames,
    // so it may land after `turn/completed`; collect until it shows up.
    let events = collect_until(&mut stream, |event| {
        matches!(event, AgentEvent::RateLimitsUpdated { .. })
    })
    .await;
    let windows = events
        .iter()
        .find_map(|event| match event {
            AgentEvent::RateLimitsUpdated { windows } => Some(windows.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no rate limits surfaced; got {events:?}"));
    assert_eq!(windows.len(), 2, "both windows surfaced: {windows:?}");
    // Windows are identified by duration, not by the server's `primary` /
    // `secondary` names: 300 minutes is a 5-hour window, 10080 a 7-day one.
    assert_eq!(windows[0].duration_seconds, Some(5 * 60 * 60));
    assert_eq!(windows[0].used_percentage, Some(21.0));
    assert_eq!(windows[0].resets_at, Some(1_700_000_000));
    assert_eq!(windows[1].duration_seconds, Some(7 * 24 * 60 * 60));
    assert_eq!(windows[1].used_percentage, Some(4.0));
}

// --- File-change approval detail ---------------------------------------------
//
// An `item/fileChange/requestApproval` carries only
// `{ itemId, startedAtMs, threadId, turnId, grantRoot?, reason? }` — no path, no
// kind, no diff. Everything the user needs to answer it travelled a moment
// earlier, on the `item/started` for the same item. These cases pin the
// correlation the adapter performs between the two, including what happens when
// it cannot be made.

/// A `fileChange` item's `changes` array, as the real server sends it.
fn file_change_item(id: &str, changes: &str) -> String {
    format!(
        r#"{{ "type": "item_started", "item": {{ "id": "{id}", "type": "fileChange",
              "status": "inProgress", "changes": [{changes}] }} }},"#
    )
}

/// One `FileUpdateChange`: its `kind` is an object tagged by `type`, not a bare
/// string.
fn change(path: &str, kind: &str, diff: &str) -> String {
    format!(r#"{{ "path": "{path}", "kind": {{ "type": "{kind}" }}, "diff": "{diff}" }}"#)
}

/// A non-blocking file-change approval naming `item_id`.
fn approval_for(item_id: &str) -> String {
    format!(
        r#"{{ "type": "request_approval", "method": "item/fileChange/requestApproval",
              "params": {{ "itemId": "{item_id}", "reason": "write access" }} }},"#
    )
}

/// The same approval, additionally asking for writes under `root` for the rest
/// of the session — the optional `grantRoot` the real params may carry.
fn approval_granting_root(item_id: &str, root: &str) -> String {
    format!(
        r#"{{ "type": "request_approval", "method": "item/fileChange/requestApproval",
              "params": {{ "itemId": "{item_id}", "reason": "write access",
                           "grantRoot": "{root}" }} }},"#
    )
}

/// Drive a scenario and return every `PermissionRequested` it raised, in order.
async fn approvals_of(scenario: &str) -> Vec<delta_usecase::AgentPermissionRequest> {
    let (adapter, _guard) = adapter_with(scenario).await;
    let handle = adapter.launch(launch_request()).await.expect("launch");
    let mut stream = adapter.events(&handle);
    adapter
        .send(
            &handle,
            SendRequest {
                text: "change a file".to_owned(),
            },
        )
        .await
        .expect("send");
    collect_until(&mut stream, |e| {
        matches!(e, AgentEvent::TurnCompleted { .. })
    })
    .await
    .into_iter()
    .filter_map(|event| match event {
        AgentEvent::PermissionRequested { request } => Some(request),
        _ => None,
    })
    .collect()
}

/// The approval arrives with the paths, kinds and diffs its item announced — the
/// whole point of the correlation, since its own params carry none of them.
#[tokio::test]
async fn a_file_change_approval_carries_its_items_paths_kinds_and_diffs() {
    let scenario = turn_scenario(&format!(
        "{}{}",
        file_change_item(
            "fc1",
            &format!(
                "{},{}",
                change("src/lib.rs", "update", "@@ -1 +1 @@"),
                change("src/new.rs", "add", "+fresh")
            ),
        ),
        approval_for("fc1"),
    ));

    let approvals = approvals_of(&scenario).await;

    let [approval] = approvals.as_slice() else {
        panic!("expected exactly one approval, got {approvals:?}");
    };
    let detail = approval
        .file_change
        .as_ref()
        .expect("the approval carries its item's file-change detail");
    assert_eq!(
        detail.changes,
        vec![
            AgentFileChange {
                path: "src/lib.rs".to_owned(),
                kind: Some(AgentFileChangeKind::Update),
                diff: "@@ -1 +1 @@".to_owned(),
            },
            AgentFileChange {
                path: "src/new.rs".to_owned(),
                kind: Some(AgentFileChangeKind::Add),
                diff: "+fresh".to_owned(),
            },
        ],
    );
    assert_eq!(
        detail.reason.as_deref(),
        Some("write access"),
        "the provider's own explanation rides the detail"
    );
    assert_eq!(
        approval.grant_root, None,
        "these params ask for no write root, so none is invented"
    );
}

/// A `grantRoot` asks for something far broader than the files the item lists:
/// writes anywhere under that root for the rest of the session. It rides the
/// approval's own params, not the item, so it must reach the card whether or not
/// the change set could be correlated — and the uncorrelated case is the one it
/// matters most in, because there the card has nothing else to show.
#[tokio::test]
async fn a_grant_root_reaches_the_card_correlated_or_not() {
    let scenario = turn_scenario(&format!(
        "{}{}{}",
        file_change_item("fc1", &change("src/lib.rs", "update", "@@ -1 +1 @@")),
        approval_granting_root("fc1", "/repo"),
        approval_granting_root("never-announced", "/elsewhere"),
    ));

    let approvals = approvals_of(&scenario).await;

    let [correlated, uncorrelated] = approvals.as_slice() else {
        panic!("expected exactly two approvals, got {approvals:?}");
    };
    assert_eq!(correlated.grant_root.as_deref(), Some("/repo"));
    assert!(
        correlated.file_change.is_some(),
        "the root does not displace the change set: both are carried"
    );
    assert_eq!(
        uncorrelated.grant_root.as_deref(),
        Some("/elsewhere"),
        "the fallback keeps the broadest ask instead of hiding it"
    );
    assert_eq!(uncorrelated.file_change, None);
}

/// Two file-change items in flight at once: each approval must get ITS item's
/// changes. A correlation that crossed them would show the user one file's diff
/// while gating another's write.
#[tokio::test]
async fn concurrent_file_change_items_never_cross_correlate() {
    let scenario = turn_scenario(&format!(
        "{}{}{}{}",
        file_change_item("fc1", &change("a.rs", "update", "diff-a")),
        file_change_item("fc2", &change("b.rs", "delete", "diff-b")),
        // Answered out of order relative to the items, so passing cannot be an
        // artefact of the two arriving in step.
        approval_for("fc2"),
        approval_for("fc1"),
    ));

    let approvals = approvals_of(&scenario).await;

    let paths: Vec<Vec<String>> = approvals
        .iter()
        .map(|approval| {
            approval
                .file_change
                .as_ref()
                .expect("each approval carries its own item's detail")
                .changes
                .iter()
                .map(|change| change.path.clone())
                .collect()
        })
        .collect();
    assert_eq!(
        paths,
        vec![vec!["b.rs".to_owned()], vec!["a.rs".to_owned()]],
        "each approval shows its own item's files: {approvals:?}"
    );
}

/// `item/fileChange/patchUpdated` restates an item's whole change set. An
/// approval raised afterwards must show the revised patch, not the one
/// `item/started` announced — otherwise the user approves a diff they were never
/// shown.
#[tokio::test]
async fn a_revised_patch_replaces_the_diff_the_approval_shows() {
    let scenario = turn_scenario(&format!(
        r#"{}{{ "type": "notification", "method": "item/fileChange/patchUpdated",
                "params": {{ "itemId": "fc1", "changes": [{}] }} }},{}"#,
        file_change_item("fc1", &change("a.rs", "update", "stale")),
        change("a.rs", "update", "revised"),
        approval_for("fc1"),
    ));

    let approvals = approvals_of(&scenario).await;

    let [approval] = approvals.as_slice() else {
        panic!("expected exactly one approval, got {approvals:?}");
    };
    assert_eq!(
        approval
            .file_change
            .as_ref()
            .expect("the approval carries the item's detail")
            .changes,
        vec![AgentFileChange {
            path: "a.rs".to_owned(),
            kind: Some(AgentFileChangeKind::Update),
            diff: "revised".to_owned(),
        }],
    );
}

/// An approval whose item was never seen (a missed notification, an out-of-order
/// frame, a session resumed mid-turn) keeps NO detail. That `None` is the
/// deliberate fallback: the card renders the request's params exactly as it did
/// before this correlation existed, rather than an empty detail block.
#[tokio::test]
async fn an_approval_with_no_known_item_falls_back_to_no_detail() {
    let scenario = turn_scenario(&approval_for("never-announced"));

    let approvals = approvals_of(&scenario).await;

    let [approval] = approvals.as_slice() else {
        panic!("expected exactly one approval, got {approvals:?}");
    };
    assert_eq!(
        approval.file_change, None,
        "an uncorrelated approval invents no detail"
    );
    assert_eq!(
        approval.tool_name, "file_change",
        "it still names the interaction by its kind"
    );
    assert_eq!(
        approval.input_json.get("itemId").and_then(|id| id.as_str()),
        Some("never-announced"),
        "and its params still ride the request, which is what the card falls back to"
    );
}

/// An item that names no file at all takes the SAME fallback as an unknown one.
///
/// A detail carrying an empty `changes` array would say nothing the input
/// summary does not, while breaking the promise both wire surfaces make — that a
/// detail, when present, names the files that would change. Every client reading
/// that contract would then need an empty-state branch it was told it would never
/// need; the browser card guards against one anyway, but the guard must not be
/// what the contract rests on.
#[tokio::test]
async fn an_item_naming_no_file_falls_back_like_an_unknown_one() {
    let scenario = turn_scenario(&format!(
        "{}{}",
        file_change_item("fc1", ""),
        approval_for("fc1"),
    ));

    let approvals = approvals_of(&scenario).await;

    let [approval] = approvals.as_slice() else {
        panic!("expected exactly one approval, got {approvals:?}");
    };
    assert_eq!(
        approval.file_change, None,
        "an empty change set is no detail, not an empty one"
    );
}

/// The correlation map is live state with a lifecycle, not a log: an entry goes
/// when its item completes and the whole map goes when the turn ends. A session
/// running for hours must not accumulate every diff it ever proposed.
#[tokio::test]
async fn the_correlation_map_is_emptied_on_item_completion_and_at_turn_end() {
    // Two items; one completes, the other is still open when the turn parks on a
    // blocking approval — so the count at that moment distinguishes "the
    // completed item was forgotten" from "nothing is tracked at all".
    let scenario = format!(
        r#"{{
            "thread_id": "thr_fc_lifecycle",
            "turn": {{
                "turn_id": "turn_fc_lifecycle",
                "emit": [
                    {{ "type": "turn_started" }},
                    {}{}
                    {{ "type": "item_completed", "item": {{ "id": "fc1", "type": "fileChange",
                       "status": "completed", "changes": [{}] }} }},
                    {{ "type": "request_approval", "blocking": true,
                       "method": "item/fileChange/requestApproval",
                       "params": {{ "itemId": "fc2" }} }},
                    {{ "type": "turn_completed", "status": "completed" }}
                ]
            }}
        }}"#,
        file_change_item("fc1", &change("a.rs", "update", "diff-a")),
        file_change_item("fc2", &change("b.rs", "update", "diff-b")),
        change("a.rs", "update", "diff-a"),
    );
    let (adapter, _guard) = adapter_with(&scenario).await;
    let handle = adapter.launch(launch_request()).await.expect("launch");
    let mut stream = adapter.events(&handle);
    adapter
        .send(
            &handle,
            SendRequest {
                text: "change files".to_owned(),
            },
        )
        .await
        .expect("send");

    let raised = collect_until(&mut stream, |e| {
        matches!(e, AgentEvent::PermissionRequested { .. })
    })
    .await;
    let request_id = raised
        .iter()
        .find_map(|e| match e {
            AgentEvent::PermissionRequested { request } => Some(request.request_id.clone()),
            _ => None,
        })
        .expect("a PermissionRequested to answer");
    assert_eq!(
        adapter.tracked_file_change_items(&handle.key),
        1,
        "the completed item was forgotten; the open one is still correlated"
    );

    let adapter_dyn: &dyn AgentAdapter = &adapter;
    adapter_dyn
        .resolve_permission(&handle, &request_id, PermissionDecision::Allow)
        .await
        .expect("resolve_permission");
    collect_until(&mut stream, |e| {
        matches!(e, AgentEvent::TurnCompleted { .. })
    })
    .await;

    assert_eq!(
        adapter.tracked_file_change_items(&handle.key),
        0,
        "the turn ending leaves nothing tracked"
    );
}

/// A lost connection releases the correlation too: the diffs are the bulky part
/// of the map, and nothing tracked can still be answered on a dead wire.
#[tokio::test]
async fn a_lost_connection_empties_the_correlation_map() {
    let scenario = format!(
        r#"{{
            "thread_id": "thr_fc_death",
            "turn": {{
                "turn_id": "turn_fc_death",
                "emit": [
                    {{ "type": "turn_started" }},
                    {}
                    {{ "type": "request_approval", "blocking": true,
                       "method": "item/fileChange/requestApproval",
                       "params": {{ "itemId": "fc1" }} }},
                    {{ "type": "exit" }}
                ]
            }}
        }}"#,
        file_change_item("fc1", &change("a.rs", "update", "diff-a")),
    );
    let (adapter, _guard) = adapter_with(&scenario).await;
    let handle = adapter.launch(launch_request()).await.expect("launch");
    let mut stream = adapter.events(&handle);
    adapter
        .send(
            &handle,
            SendRequest {
                text: "change a file".to_owned(),
            },
        )
        .await
        .expect("send");

    let raised = collect_until(&mut stream, |e| {
        matches!(e, AgentEvent::PermissionRequested { .. })
    })
    .await;
    let request_id = raised
        .iter()
        .find_map(|e| match e {
            AgentEvent::PermissionRequested { request } => Some(request.request_id.clone()),
            _ => None,
        })
        .expect("a PermissionRequested to answer");
    assert_eq!(
        adapter.tracked_file_change_items(&handle.key),
        1,
        "the item is correlated while the approval is open"
    );

    // Answering releases the fake, which then exits where a real app-server
    // would die: the client sees the EOF as a lost connection.
    let adapter_dyn: &dyn AgentAdapter = &adapter;
    adapter_dyn
        .resolve_permission(&handle, &request_id, PermissionDecision::Allow)
        .await
        .expect("resolve_permission");
    collect_until(&mut stream, |e| {
        matches!(
            e,
            AgentEvent::SessionEnded {
                reason: SessionEndReason::ProcessExited
            }
        )
    })
    .await;

    assert_eq!(
        adapter.tracked_file_change_items(&handle.key),
        0,
        "the death released the tracked diffs"
    );
}
