//! Provider-neutral adapter contract test harness, run against the Claude
//! adapter.
//!
//! The cases here are written generically over any [`AgentAdapter`] and read
//! the adapter's declared [`AgentCapabilities`] to decide what to assert — so
//! the same suite runs against a future structured adapter (Codex) unchanged.
//! In this phase there is one implementor, so the harness lives in this crate;
//! when a second adapter arrives it lifts verbatim into a shared test-support
//! crate (the case functions take `&impl AgentAdapter` precisely so that move
//! is mechanical).
//!
//! ## Active vs. pending
//!
//! The Claude adapter drives the *mechanical* side of the session (launch,
//! send, interrupt, close) and emits the events it produces at those points
//! ([`AgentEvent::SessionStarted`], [`AgentEvent::UserPromptAccepted`],
//! [`AgentEvent::SessionEnded`]). Those cases run and pass.
//!
//! The cases that assert turn/permission lifecycle events
//! ([`AgentEvent::TurnStarted`], [`AgentEvent::TurnCompleted`],
//! [`AgentEvent::PermissionRequested`]/`Resolved`,
//! [`AgentEvent::UnsupportedInteraction`]) depend on the core routing Claude's
//! hooks and transcript into the `AgentEvent` stream — a later phase. Until
//! then they are `#[ignore]`d scaffolds with the intended assertion in the
//! body, rather than faked passes. Each drops the adapter before draining so an
//! accidentally-un-ignored scaffold fails fast instead of hanging.

use async_trait::async_trait;
use std::sync::Mutex;

use delta_usecase::{
    AgentAdapter, AgentEvent, AgentEventStream, AgentProvider, ContextInjectionCapability,
    InterruptCapability, LaunchRequest, Result, SendRequest, SessionEndReason, TmuxDriver,
};

use crate::{ClaudeCodePtyHookAdapter, ClaudeLaunchConfig};

// --- Recording tmux fake ----------------------------------------------------

/// A single recorded `create_session` call.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CreatedSession {
    name: String,
    workdir: String,
    command: Vec<String>,
}

/// A [`TmuxDriver`] that records every call, so the Claude-specific tests can
/// assert the exact commands and keystrokes without a real tmux.
#[derive(Default)]
struct RecordingTmux {
    created: Mutex<Vec<CreatedSession>>,
    sent: Mutex<Vec<(String, String)>>,
    keyed: Mutex<Vec<(String, Vec<String>)>>,
    killed: Mutex<Vec<String>>,
}

#[async_trait]
impl TmuxDriver for RecordingTmux {
    async fn has_session(&self, name: &str) -> Result<bool> {
        Ok(self.created.lock().unwrap().iter().any(|c| c.name == name))
    }

    async fn create_session(&self, name: &str, workdir: &str, command: &[String]) -> Result<()> {
        self.created.lock().unwrap().push(CreatedSession {
            name: name.to_owned(),
            workdir: workdir.to_owned(),
            command: command.to_vec(),
        });
        Ok(())
    }

    async fn send_line(&self, pane: &str, text: &str) -> Result<()> {
        self.sent
            .lock()
            .unwrap()
            .push((pane.to_owned(), text.to_owned()));
        Ok(())
    }

    async fn send_keys(&self, pane: &str, keys: &[&str]) -> Result<()> {
        self.keyed.lock().unwrap().push((
            pane.to_owned(),
            keys.iter().map(|k| (*k).to_owned()).collect(),
        ));
        Ok(())
    }

    async fn clear_input(&self, _pane: &str) -> Result<()> {
        Ok(())
    }

    async fn kill_session(&self, name: &str) -> Result<()> {
        self.killed.lock().unwrap().push(name.to_owned());
        Ok(())
    }
}

// --- Fixtures ---------------------------------------------------------------

fn claude_adapter() -> ClaudeCodePtyHookAdapter<RecordingTmux> {
    ClaudeCodePtyHookAdapter::new(
        RecordingTmux::default(),
        ClaudeLaunchConfig {
            claude_bin: "claude".to_owned(),
            settings_path: "/tmp/delta-settings.json".to_owned(),
        },
    )
}

fn launch_request() -> LaunchRequest {
    LaunchRequest {
        session_id: "01920000-0000-7000-8000-000000000001".to_owned(),
        workdir: "/tmp/workdir".to_owned(),
        extra_args: Vec::new(),
        first_prompt: None,
    }
}

/// Drain every event buffered on `stream`. The caller must have dropped the
/// adapter first so the channel is closed and this returns rather than blocks.
async fn drain(stream: &mut AgentEventStream) -> Vec<AgentEvent> {
    let mut events = Vec::new();
    while let Some(event) = stream.recv().await {
        events.push(event);
    }
    events
}

// --- Provider-neutral cases (active) ----------------------------------------

/// `launch_returns_provider_session_id`: launching yields a handle carrying a
/// provider session id, and the stream opens with a matching `SessionStarted`.
async fn case_launch_returns_provider_session_id<A: AgentAdapter>(adapter: &A) {
    let handle = adapter.launch(launch_request()).await.expect("launch");
    assert!(
        !handle.provider_session_id.is_empty(),
        "launch must return a provider session id"
    );
    let mut stream = adapter.events(&handle);
    match stream.recv().await {
        Some(AgentEvent::SessionStarted {
            provider_session_id,
        }) => assert_eq!(provider_session_id, handle.provider_session_id),
        other => panic!("expected SessionStarted, got {other:?}"),
    }
}

/// `send_emits_user_prompt_accepted`: a send surfaces the prompt as an accepted
/// user prompt on the event stream (the mechanical half of
/// `send_emits_user_prompt_and_turn_started`; the `TurnStarted` half is the
/// pending scaffold below).
async fn case_send_emits_user_prompt_accepted<A: AgentAdapter>(adapter: &A) {
    let handle = adapter.launch(launch_request()).await.expect("launch");
    let mut stream = adapter.events(&handle);
    assert!(matches!(
        stream.recv().await,
        Some(AgentEvent::SessionStarted { .. })
    ));
    adapter
        .send(
            &handle,
            SendRequest {
                text: "hello agent".to_owned(),
            },
        )
        .await
        .expect("send");
    match stream.recv().await {
        Some(AgentEvent::UserPromptAccepted { text, .. }) => assert_eq!(text, "hello agent"),
        other => panic!("expected UserPromptAccepted, got {other:?}"),
    }
}

/// `context_injection_does_not_pollute_visible_prompt` (asserted only for
/// `HiddenPerTurn` providers): the visible prompt the adapter reports is
/// exactly what was sent — no hidden context is folded into it (Claude injects
/// hidden context out of band via the `UserPromptSubmit` hook).
async fn case_context_injection_does_not_pollute_visible_prompt<A: AgentAdapter>(adapter: &A) {
    if adapter.capabilities().context_injection != ContextInjectionCapability::HiddenPerTurn {
        return;
    }
    let handle = adapter.launch(launch_request()).await.expect("launch");
    let mut stream = adapter.events(&handle);
    assert!(matches!(
        stream.recv().await,
        Some(AgentEvent::SessionStarted { .. })
    ));
    let visible = "just the user's words";
    adapter
        .send(
            &handle,
            SendRequest {
                text: visible.to_owned(),
            },
        )
        .await
        .expect("send");
    match stream.recv().await {
        Some(AgentEvent::UserPromptAccepted { text, .. }) => assert_eq!(
            text, visible,
            "the visible prompt must not carry injected context"
        ),
        other => panic!("expected UserPromptAccepted, got {other:?}"),
    }
}

/// `interrupt_is_accepted_when_supported` (skipped when the provider declares
/// `InterruptCapability::Unsupported`): interrupting an open session succeeds.
/// The turn-ending event it produces is the pending `interrupt_ends_turn`
/// scaffold below.
async fn case_interrupt_is_accepted_when_supported<A: AgentAdapter>(adapter: &A) {
    if adapter.capabilities().interrupt == InterruptCapability::Unsupported {
        return;
    }
    let handle = adapter.launch(launch_request()).await.expect("launch");
    adapter.interrupt(&handle).await.expect("interrupt");
}

/// `close_ends_the_session`: closing an open session succeeds and emits
/// `SessionEnded`.
async fn case_close_ends_the_session<A: AgentAdapter>(adapter: &A) {
    let handle = adapter.launch(launch_request()).await.expect("launch");
    let mut stream = adapter.events(&handle);
    assert!(matches!(
        stream.recv().await,
        Some(AgentEvent::SessionStarted { .. })
    ));
    adapter.close(&handle).await.expect("close");
    match stream.recv().await {
        Some(AgentEvent::SessionEnded { reason }) => {
            assert_eq!(reason, SessionEndReason::Closed)
        }
        other => panic!("expected SessionEnded, got {other:?}"),
    }
}

// --- Claude adapter: active cases -------------------------------------------

#[tokio::test]
async fn launch_returns_provider_session_id() {
    case_launch_returns_provider_session_id(&claude_adapter()).await;
}

#[tokio::test]
async fn send_emits_user_prompt_accepted() {
    case_send_emits_user_prompt_accepted(&claude_adapter()).await;
}

#[tokio::test]
async fn context_injection_does_not_pollute_visible_prompt() {
    case_context_injection_does_not_pollute_visible_prompt(&claude_adapter()).await;
}

#[tokio::test]
async fn interrupt_is_accepted_when_supported() {
    case_interrupt_is_accepted_when_supported(&claude_adapter()).await;
}

#[tokio::test]
async fn close_ends_the_session() {
    case_close_ends_the_session(&claude_adapter()).await;
}

/// Claude-specific: the launch command line is the exact spawn shape Claude
/// needs (`--settings <path> --session-id <id>`, then the positional prompt),
/// and it is the Delta-minted id that becomes the provider session id. This
/// pins the "change dependency direction, not behaviour" property of the
/// adapter.
#[tokio::test]
async fn claude_launch_builds_the_expected_command() {
    let adapter = ClaudeCodePtyHookAdapter::new(
        RecordingTmux::default(),
        ClaudeLaunchConfig {
            claude_bin: "claude".to_owned(),
            settings_path: "/settings.json".to_owned(),
        },
    );
    let handle = adapter
        .launch(LaunchRequest {
            session_id: "sid-123".to_owned(),
            workdir: "/work".to_owned(),
            extra_args: vec!["--model".to_owned(), "opus".to_owned()],
            first_prompt: Some("do the thing".to_owned()),
        })
        .await
        .expect("launch");

    assert_eq!(handle.provider, AgentProvider::Claude);
    assert_eq!(handle.provider_session_id, "sid-123");

    // The `contract` module is part of this crate, so it can inspect the
    // adapter's owned tmux fake directly.
    let created = adapter.tmux.created.lock().unwrap();
    assert_eq!(created.len(), 1);
    assert_eq!(created[0].name, handle.key);
    assert_eq!(created[0].workdir, "/work");
    assert_eq!(
        created[0].command,
        vec![
            "claude".to_owned(),
            "--settings".to_owned(),
            "/settings.json".to_owned(),
            "--session-id".to_owned(),
            "sid-123".to_owned(),
            "--model".to_owned(),
            "opus".to_owned(),
            "do the thing".to_owned(),
        ]
    );
}

// --- Claude adapter: pending (Phase B) scaffolds ----------------------------
//
// These assert events the adapter cannot yet emit because Claude's hooks and
// transcript are not routed into the `AgentEvent` stream in this phase. They
// are real scaffolds (the intended assertion is in the body); when the event
// projection lands, drop the `#[ignore]`.

/// `send_emits_user_prompt_and_turn_started`.
#[tokio::test]
#[ignore = "Phase B: TurnStarted derives from the UserPromptSubmit echo, not yet projected"]
async fn send_emits_user_prompt_and_turn_started() {
    let adapter = claude_adapter();
    let handle = adapter.launch(launch_request()).await.expect("launch");
    let mut stream = adapter.events(&handle);
    adapter
        .send(
            &handle,
            SendRequest {
                text: "hello".to_owned(),
            },
        )
        .await
        .expect("send");
    drop(adapter);
    let events = drain(&mut stream).await;
    assert!(
        events
            .iter()
            .any(|e| matches!(e, AgentEvent::UserPromptAccepted { .. })),
        "expected UserPromptAccepted"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, AgentEvent::TurnStarted { .. })),
        "expected TurnStarted once the prompt echo is projected"
    );
}

/// `turn_completion_is_emitted_once`.
#[tokio::test]
#[ignore = "Phase B: TurnCompleted derives from the Stop hook, not yet projected"]
async fn turn_completion_is_emitted_once() {
    let adapter = claude_adapter();
    let handle = adapter.launch(launch_request()).await.expect("launch");
    let mut stream = adapter.events(&handle);
    adapter
        .send(
            &handle,
            SendRequest {
                text: "hello".to_owned(),
            },
        )
        .await
        .expect("send");
    drop(adapter);
    let events = drain(&mut stream).await;
    let completions = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::TurnCompleted { .. }))
        .count();
    assert_eq!(completions, 1, "exactly one TurnCompleted per turn");
}

/// `permission_request_can_be_allowed`.
#[tokio::test]
#[ignore = "Phase B: permission flow is not yet routed through AgentEvent"]
async fn permission_request_can_be_allowed() {
    let adapter = claude_adapter();
    let handle = adapter.launch(launch_request()).await.expect("launch");
    let mut stream = adapter.events(&handle);
    drop(adapter);
    let events = drain(&mut stream).await;
    assert!(
        events
            .iter()
            .any(|e| matches!(e, AgentEvent::PermissionRequested { .. })),
        "expected a PermissionRequested to allow"
    );
    assert!(
        events.iter().any(|e| matches!(
            e,
            AgentEvent::PermissionResolved {
                decision: delta_usecase::PermissionDecision::Allow,
                ..
            }
        )),
        "expected PermissionResolved(Allow)"
    );
}

/// `permission_request_can_be_denied`.
#[tokio::test]
#[ignore = "Phase B: permission flow is not yet routed through AgentEvent"]
async fn permission_request_can_be_denied() {
    let adapter = claude_adapter();
    let handle = adapter.launch(launch_request()).await.expect("launch");
    let mut stream = adapter.events(&handle);
    drop(adapter);
    let events = drain(&mut stream).await;
    assert!(
        events.iter().any(|e| matches!(
            e,
            AgentEvent::PermissionResolved {
                decision: delta_usecase::PermissionDecision::Deny,
                ..
            }
        )),
        "expected PermissionResolved(Deny)"
    );
}

/// `interrupt_ends_turn`.
#[tokio::test]
#[ignore = "Phase B: TurnCompleted(Interrupted) derives from the transcript marker"]
async fn interrupt_ends_turn() {
    let adapter = claude_adapter();
    let handle = adapter.launch(launch_request()).await.expect("launch");
    let mut stream = adapter.events(&handle);
    adapter.interrupt(&handle).await.expect("interrupt");
    drop(adapter);
    let events = drain(&mut stream).await;
    assert!(
        events.iter().any(|e| matches!(
            e,
            AgentEvent::TurnCompleted {
                status: delta_usecase::TurnStatus::Interrupted
            }
        )),
        "expected TurnCompleted(Interrupted)"
    );
}

/// `no_server_request_silently_hangs`: any server-to-client interaction Delta
/// does not model must surface as `UnsupportedInteraction` rather than block.
#[tokio::test]
#[ignore = "Phase B/C: the UnsupportedInteraction surfacing lands with structured events"]
async fn no_server_request_silently_hangs() {
    let adapter = claude_adapter();
    let handle = adapter.launch(launch_request()).await.expect("launch");
    let mut stream = adapter.events(&handle);
    drop(adapter);
    let events = drain(&mut stream).await;
    assert!(
        events
            .iter()
            .any(|e| matches!(e, AgentEvent::UnsupportedInteraction { .. })),
        "an unmodeled server request must surface as UnsupportedInteraction"
    );
}
