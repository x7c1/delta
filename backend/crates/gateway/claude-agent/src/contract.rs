//! The provider-neutral adapter contract suite, run against the Claude adapter.
//!
//! The suite itself lives in the shared [`agent_contract`] crate; every case is
//! written generically over `&impl AgentAdapter` so the Codex adapter runs the
//! identical bodies. This module supplies the Claude fixture and calls each
//! shared case, then adds the Claude-specific cases whose *stimulus* is
//! provider-specific.
//!
//! ## Shared cases (mechanical, driven through the trait)
//!
//! Launch/send/interrupt/close and the events those operations emit directly
//! ([`AgentEvent::SessionStarted`], [`AgentEvent::UserPromptAccepted`],
//! [`AgentEvent::SessionEnded`]) come from [`agent_contract`].
//!
//! ## Claude-specific cases (driven through the ingestion seam)
//!
//! The **permission** and **turn-lifecycle** cases drive the adapter's ingestion
//! seam ([`ClaudeCodePtyHookAdapter::ingest_hook`] +
//! [`ClaudeCodePtyHookAdapter::ingest_transcript_lines`]) — Claude's lossy input
//! is fed in explicitly, since there is no live hook/transcript source in a unit
//! test. They project:
//!
//! - a `PermissionRequest` hook and its correlated `tool_result` into
//!   [`AgentEvent::PermissionRequested`]/[`AgentEvent::PermissionResolved`];
//! - a `UserPromptSubmit` echo into [`AgentEvent::TurnStarted`] (its
//!   `UserPromptAccepted` half coming from `send`);
//! - a `Stop` hook into [`AgentEvent::TurnCompleted`]`(Completed)`;
//! - the `[Request interrupted by user…]` transcript marker into
//!   [`AgentEvent::TurnCompleted`]`(Interrupted)`.
//!
//! The `no_server_request_silently_hangs` case asserts
//! [`AgentEvent::UnsupportedInteraction`], which depends on structured
//! server→client requests Claude does not emit; it is exercised meaningfully by
//! the Codex adapter instead. Here it stays an `#[ignore]`d scaffold with the
//! intended assertion in the body, rather than a faked pass. It drops the
//! adapter before draining so an accidentally-un-ignored scaffold fails fast
//! instead of hanging.

use async_trait::async_trait;
use std::sync::Mutex;

use agent_contract::{drain, launch_request};
use delta_usecase::{
    AgentAdapter, AgentEvent, AgentProvider, LaunchRequest, Result, SendRequest, TmuxDriver,
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

// --- Shared provider-neutral cases (run against the Claude adapter) ---------

#[tokio::test]
async fn launch_returns_provider_session_id() {
    agent_contract::case_launch_returns_provider_session_id(&claude_adapter()).await;
}

#[tokio::test]
async fn send_emits_user_prompt_accepted() {
    agent_contract::case_send_emits_user_prompt_accepted(&claude_adapter()).await;
}

#[tokio::test]
async fn context_injection_does_not_pollute_visible_prompt() {
    agent_contract::case_context_injection_does_not_pollute_visible_prompt(&claude_adapter()).await;
}

#[tokio::test]
async fn interrupt_is_accepted_when_supported() {
    agent_contract::case_interrupt_is_accepted_when_supported(&claude_adapter()).await;
}

#[tokio::test]
async fn close_ends_the_session() {
    agent_contract::case_close_ends_the_session(&claude_adapter()).await;
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
// These assert turn-lifecycle events the ingestion seam does not project yet
// (only the permission projection is wired in this phase). They are real
// scaffolds (the intended assertion is in the body); when the prompt/turn
// projections land, drop the `#[ignore]`.

/// `send_emits_user_prompt_and_turn_started`: a send surfaces the prompt as an
/// accepted user prompt (the mechanical dispatch fact), and the resulting
/// `UserPromptSubmit` echo — fed through the ingestion seam — projects
/// `TurnStarted`.
#[tokio::test]
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
    // Claude confirms the turn by echoing the prompt through the
    // `UserPromptSubmit` hook; that echo is the turn-start signal.
    adapter
        .ingest_hook(&handle, r#"{"hook":"user_prompt_submit","prompt":"hello"}"#)
        .expect("ingest user_prompt_submit hook");
    drop(adapter);
    let events = drain(&mut stream).await;
    assert!(
        events
            .iter()
            .any(|e| matches!(e, AgentEvent::UserPromptAccepted { .. })),
        "expected UserPromptAccepted, got {events:?}"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, AgentEvent::TurnStarted { .. })),
        "expected TurnStarted once the prompt echo is projected, got {events:?}"
    );
}

/// `turn_completion_is_emitted_once`: the `Stop` hook fed through the seam
/// projects exactly one `TurnCompleted` for the turn.
#[tokio::test]
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
    adapter
        .ingest_hook(&handle, r#"{"hook":"stop"}"#)
        .expect("ingest stop hook");
    drop(adapter);
    let events = drain(&mut stream).await;
    let completions = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::TurnCompleted { .. }))
        .count();
    assert_eq!(completions, 1, "exactly one TurnCompleted per turn");
}

/// `permission_request_can_be_allowed`: a `PermissionRequest` hook fed through
/// the ingestion seam projects `PermissionRequested`, and the correlated
/// `tool_result` (no error → allowed) projects `PermissionResolved(Allow)` for
/// the same minted request id.
#[tokio::test]
async fn permission_request_can_be_allowed() {
    let adapter = claude_adapter();
    let handle = adapter.launch(launch_request()).await.expect("launch");
    let mut stream = adapter.events(&handle);
    assert!(matches!(
        stream.recv().await,
        Some(AgentEvent::SessionStarted { .. })
    ));

    adapter
        .ingest_hook(
            &handle,
            r#"{"hook":"permission_request","tool_name":"Bash",
                "tool_input":{"command":"rm -i x"},"tool_use_id":"tu-allow"}"#,
        )
        .expect("ingest permission-request hook");
    // The correlated tool_result carries no error, so the call was allowed.
    adapter.ingest_transcript_lines(
        &handle,
        &[r#"{"type":"tool_result","tool_use_id":"tu-allow"}"#.to_owned()],
    );

    drop(adapter);
    let events = drain(&mut stream).await;

    let requested_id = events.iter().find_map(|e| match e {
        AgentEvent::PermissionRequested { request } => Some(request.request_id.clone()),
        _ => None,
    });
    let requested_id = requested_id.expect("expected a PermissionRequested to allow");
    assert!(
        events.iter().any(|e| matches!(
            e,
            AgentEvent::PermissionResolved {
                request_id,
                decision: delta_usecase::PermissionDecision::Allow,
            } if *request_id == requested_id
        )),
        "expected PermissionResolved(Allow) for the requested id, got {events:?}"
    );
}

/// `permission_request_can_be_denied`: the same seam, but the correlated
/// `tool_result` is an error, which projects `PermissionResolved(Deny)`.
#[tokio::test]
async fn permission_request_can_be_denied() {
    let adapter = claude_adapter();
    let handle = adapter.launch(launch_request()).await.expect("launch");
    let mut stream = adapter.events(&handle);
    assert!(matches!(
        stream.recv().await,
        Some(AgentEvent::SessionStarted { .. })
    ));

    adapter
        .ingest_hook(
            &handle,
            r#"{"hook":"permission_request","tool_name":"Bash",
                "tool_input":{"command":"rm -rf /"},"tool_use_id":"tu-deny"}"#,
        )
        .expect("ingest permission-request hook");
    // An errored tool_result is the deny signal.
    adapter.ingest_transcript_lines(
        &handle,
        &[r#"{"type":"tool_result","tool_use_id":"tu-deny","is_error":true}"#.to_owned()],
    );

    drop(adapter);
    let events = drain(&mut stream).await;

    assert!(
        events
            .iter()
            .any(|e| matches!(e, AgentEvent::PermissionRequested { .. })),
        "expected a PermissionRequested to deny, got {events:?}"
    );
    assert!(
        events.iter().any(|e| matches!(
            e,
            AgentEvent::PermissionResolved {
                decision: delta_usecase::PermissionDecision::Deny,
                ..
            }
        )),
        "expected PermissionResolved(Deny), got {events:?}"
    );
}

/// `interrupt_ends_turn`: interrupting injects the abort, and the
/// `[Request interrupted by user…]` marker it leaves in the transcript — fed
/// through the seam — projects `TurnCompleted(Interrupted)` (no `Stop` fires).
#[tokio::test]
async fn interrupt_ends_turn() {
    let adapter = claude_adapter();
    let handle = adapter.launch(launch_request()).await.expect("launch");
    let mut stream = adapter.events(&handle);
    adapter.interrupt(&handle).await.expect("interrupt");
    // The abort is confirmed by the interrupt marker Claude writes to the
    // transcript, not by a `Stop` hook.
    adapter.ingest_transcript_lines(
        &handle,
        &[r#"{"type":"user","text":"[Request interrupted by user]"}"#.to_owned()],
    );
    drop(adapter);
    let events = drain(&mut stream).await;
    assert!(
        events.iter().any(|e| matches!(
            e,
            AgentEvent::TurnCompleted {
                status: delta_usecase::TurnStatus::Interrupted
            }
        )),
        "expected TurnCompleted(Interrupted), got {events:?}"
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
