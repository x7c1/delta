//! Golden mapping test: locks the Claude hook/transcript → [`AgentEvent`]
//! projection against silent drift.
//!
//! The [`contract`] harness proves each projection is *reachable* through the
//! adapter; this suite pins the *exact* [`AgentEvent`] sequence a scripted
//! session emits, so a change to what the ingestion seam projects (a reordered,
//! dropped, added, or reshaped event) fails loudly rather than sliding through
//! green. A Claude wording/shape change that the seam silently stops matching
//! shows up here as a missing event in the diff.
//!
//! ## What is fed in
//!
//! Each scenario drives a fresh adapter's ingestion seam
//! ([`ClaudeCodePtyHookAdapter::ingest_hook`] +
//! [`ClaudeCodePtyHookAdapter::ingest_transcript_lines`]) with a scripted list
//! of wire inputs and records the full [`AgentEvent`] stream (the launch
//! `SessionStarted` opener included, so the golden is the complete observed
//! sequence). No `send`/`interrupt`/`close` is called: those emit the adapter's
//! *mechanical* events, which the contract harness already covers — this suite
//! isolates the hook/transcript **projection**.
//!
//! ## Projections covered
//!
//! - `user_prompt_submit` hook → [`AgentEvent::TurnStarted`];
//! - `permission_request` hook → [`AgentEvent::PermissionRequested`];
//! - correlated `tool_result` → [`AgentEvent::PermissionResolved`] (allow when
//!   no error, deny when `is_error`);
//! - `stop` hook → [`AgentEvent::TurnCompleted`]`(Completed)`;
//! - `[Request interrupted by user…]` transcript marker →
//!   [`AgentEvent::TurnCompleted`]`(Interrupted)`;
//! - unmodeled transcript lines (blank, non-JSON, records this phase does not
//!   model, an uncorrelated `tool_result`) and an unmodeled hook project to
//!   **nothing** — they are skipped, not surfaced as
//!   [`AgentEvent::UnsupportedInteraction`] (that stays a later phase).
//!
//! ## Fixture provenance
//!
//! The wire literals are the seam's own gateway-side shapes, reused from the
//! contract harness and the [`ingest`](crate::ingest) unit tests rather than
//! invented here. Those shapes are the deliberately-minimal projection of the
//! real Claude wire: the raw hook POST bodies are separately pinned by
//! `delta_wire::hooks` and the real-claude canary, and the raw JSONL transcript
//! (with `tool_result`/text nested under `message.content`) is separately
//! pinned by the attribution golden corpus. The tool name/input and the
//! `[Request interrupted by user]` marker text mirror those recorded fixtures
//! (`corpus/cases/tool_results`, `corpus/cases/interrupt_mid_branch`, and
//! `claude_format::is_interrupt_marker`). The unmodeled `pre_tool_use` hook name
//! is a real Claude hook this seam does not model yet.
//!
//! ## Determinism & regeneration
//!
//! Each scenario uses a fresh adapter, so the minted pane token is always
//! `delta-1` and the minted permission id is always `delta-1-perm-1`; the fixed
//! `SESSION_ID` pins the opener. Events drain FIFO off a single thread. The
//! expected output is checked in at `tests/golden/hook_transcript_mapping.json`;
//! run with `UPDATE_GOLDEN=1` to rewrite it from the current projection, then
//! review the diff.

use std::path::PathBuf;

use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;

use delta_usecase::{
    AgentAdapter, AgentEvent, AgentEventStream, LaunchRequest, PermissionDecision, Result,
    SessionEndReason, TmuxDriver, TurnStatus,
};

use crate::{ClaudeCodePtyHookAdapter, ClaudeLaunchConfig};

/// The pinned session id every scenario launches under, so the opening
/// `SessionStarted` is deterministic.
const SESSION_ID: &str = "01920000-0000-7000-8000-000000000001";

// --- No-op tmux -------------------------------------------------------------

/// A [`TmuxDriver`] that accepts every call. The golden exercises only the
/// ingestion seam, so nothing about the driving side matters beyond a launch
/// that succeeds enough to open the event channel.
struct NoopTmux;

#[async_trait]
impl TmuxDriver for NoopTmux {
    async fn has_session(&self, _name: &str) -> Result<bool> {
        Ok(false)
    }
    async fn create_session(&self, _name: &str, _workdir: &str, _command: &[String]) -> Result<()> {
        Ok(())
    }
    async fn send_line(&self, _pane: &str, _text: &str) -> Result<()> {
        Ok(())
    }
    async fn send_keys(&self, _pane: &str, _keys: &[&str]) -> Result<()> {
        Ok(())
    }
    async fn clear_input(&self, _pane: &str) -> Result<()> {
        Ok(())
    }
    async fn kill_session(&self, _name: &str) -> Result<()> {
        Ok(())
    }
}

fn adapter() -> ClaudeCodePtyHookAdapter<NoopTmux> {
    ClaudeCodePtyHookAdapter::new(
        NoopTmux,
        ClaudeLaunchConfig {
            claude_bin: "claude".to_owned(),
            settings_path: "/tmp/delta-settings.json".to_owned(),
        },
    )
}

// --- Scenario scripting -----------------------------------------------------

/// One scripted wire input fed to the seam.
enum Step {
    /// A modeled hook payload; ingestion must accept it.
    Hook(&'static str),
    /// A hook payload this seam does not model; ingestion must reject it
    /// (projecting nothing), never surface it.
    UnmodeledHook(&'static str),
    /// A batch of transcript lines fed in one call, exactly as the reader tails
    /// them. Modeled lines project; the rest are skipped.
    Transcript(&'static [&'static str]),
}

struct Scenario {
    name: &'static str,
    steps: &'static [Step],
}

/// The scripted sessions the golden pins. Every projection the seam performs
/// today appears at least once, alongside inputs that must project nothing.
fn scenarios() -> Vec<Scenario> {
    vec![
        // A full permitted turn: prompt echo starts the turn, a gated Bash call
        // raises a dialog, its (clean) tool_result allows it, Stop ends the
        // turn. An unmodeled assistant line sits between request and result to
        // prove interleaved noise is skipped without disturbing correlation.
        Scenario {
            name: "permission_allowed_turn",
            steps: &[
                Step::Hook(r#"{"hook":"user_prompt_submit","prompt":"list the files"}"#),
                Step::Hook(
                    r#"{"hook":"permission_request","tool_name":"Bash",
                        "tool_input":{"command":"ls"},"tool_use_id":"tu-allow"}"#,
                ),
                Step::Transcript(&[
                    r#"{"type":"assistant","text":"running it"}"#,
                    r#"{"type":"tool_result","tool_use_id":"tu-allow","is_error":false}"#,
                ]),
                Step::Hook(r#"{"hook":"stop"}"#),
            ],
        },
        // The same seam, but the tool_result is an error — the deny signal.
        Scenario {
            name: "permission_denied_turn",
            steps: &[
                Step::Hook(r#"{"hook":"user_prompt_submit","prompt":"delete everything"}"#),
                Step::Hook(
                    r#"{"hook":"permission_request","tool_name":"Bash",
                        "tool_input":{"command":"rm -rf /"},"tool_use_id":"tu-deny"}"#,
                ),
                Step::Transcript(&[
                    r#"{"type":"tool_result","tool_use_id":"tu-deny","is_error":true}"#,
                ]),
                Step::Hook(r#"{"hook":"stop"}"#),
            ],
        },
        // A turn the user aborts: the interrupt marker ends it with an
        // Interrupted status and no Stop hook fires.
        Scenario {
            name: "interrupted_turn",
            steps: &[
                Step::Hook(r#"{"hook":"user_prompt_submit","prompt":"explore the side topic"}"#),
                Step::Transcript(&[r#"{"type":"user","text":"[Request interrupted by user]"}"#]),
            ],
        },
        // Nothing modeled: an unmodeled hook and a batch of transcript lines
        // that all fail to match must project no events at all (only the launch
        // opener remains). A tool_result with no open dialog is among them, so
        // an uncorrelated result stays silent too.
        Scenario {
            name: "unmodeled_wire_is_skipped",
            steps: &[
                Step::UnmodeledHook(r#"{"hook":"pre_tool_use","tool_name":"Bash"}"#),
                Step::Transcript(&[
                    "",
                    "not json",
                    r#"{"type":"assistant","text":"[Request interrupted by user]"}"#,
                    r#"{"type":"user","text":"a normal prompt, not the marker"}"#,
                    r#"{"type":"tool_result","tool_use_id":"tu-orphan"}"#,
                ]),
            ],
        },
    ]
}

// --- Golden shape -----------------------------------------------------------

/// A serializable mirror of [`AgentEvent`]. The `From<&AgentEvent>` conversion
/// is an exhaustive match, so adding an [`AgentEvent`] variant fails to compile
/// until it is mapped here — the projection surface cannot grow silently.
#[derive(Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
enum GoldenEvent {
    SessionStarted {
        provider_session_id: String,
    },
    SessionEnded {
        reason: &'static str,
    },
    TurnStarted {
        provider_turn_id: Option<String>,
    },
    UserPromptAccepted {
        provider_message_id: Option<String>,
        text: String,
    },
    AssistantDelta {
        provider_item_id: String,
        text: String,
    },
    AssistantMessage {
        provider_item_id: String,
        text: String,
    },
    /// Mirrors [`AgentEvent::ThinkingDelta`]. Unreachable from this seam — Claude
    /// never emits it (its thinking arrives already folded into the transcript's
    /// message content) — but mapped so the exhaustive match keeps compiling and
    /// a future Claude thinking projection cannot slip in ungoldened.
    ThinkingDelta {
        provider_item_id: String,
        text: String,
    },
    /// Mirrors [`AgentEvent::ThinkingMessage`]. Unreachable from this seam, for
    /// the same reason as [`GoldenEvent::ThinkingDelta`].
    ThinkingMessage {
        provider_item_id: String,
        text: String,
    },
    ToolStarted {
        provider_item_id: String,
        name: String,
        input_json: Value,
    },
    ToolCompleted {
        provider_item_id: String,
        output_json: Value,
    },
    PermissionRequested {
        request_id: String,
        tool_name: String,
        input_json: Value,
        tool_use_id: Option<String>,
    },
    PermissionResolved {
        request_id: String,
        decision: &'static str,
    },
    UnsupportedInteraction {
        method: String,
        detail_json: Value,
    },
    /// Mirrors [`AgentEvent::TokenUsageUpdated`]. Unreachable from this seam —
    /// Claude reports its usage through the status-line hook, which is not part
    /// of the adapter's event projection — but mapped so the exhaustive match
    /// keeps compiling and a future Claude usage projection cannot slip in
    /// ungoldened.
    TokenUsageUpdated {
        context_used_percentage: Option<f64>,
        context_window_size: Option<u64>,
        context_current_usage: Option<u64>,
        total_input_tokens: Option<u64>,
    },
    /// Mirrors [`AgentEvent::RateLimitsUpdated`]. Unreachable from this seam,
    /// for the same reason as [`GoldenEvent::TokenUsageUpdated`].
    RateLimitsUpdated {
        windows: Vec<GoldenRateLimitWindow>,
    },
    TurnCompleted {
        status: &'static str,
    },
    Error {
        recoverable: bool,
        message: String,
    },
}

/// A serializable mirror of one neutral rate-limit window, for
/// [`GoldenEvent::RateLimitsUpdated`].
#[derive(Serialize)]
struct GoldenRateLimitWindow {
    duration_seconds: Option<i64>,
    used_percentage: Option<f64>,
    resets_at: Option<i64>,
}

impl From<&AgentEvent> for GoldenEvent {
    fn from(event: &AgentEvent) -> Self {
        match event {
            AgentEvent::SessionStarted {
                provider_session_id,
            } => GoldenEvent::SessionStarted {
                provider_session_id: provider_session_id.clone(),
            },
            AgentEvent::SessionEnded { reason } => GoldenEvent::SessionEnded {
                reason: match reason {
                    SessionEndReason::Closed => "closed",
                    SessionEndReason::ProcessExited => "process_exited",
                    SessionEndReason::Failed => "failed",
                },
            },
            AgentEvent::TurnStarted { provider_turn_id } => GoldenEvent::TurnStarted {
                provider_turn_id: provider_turn_id.clone(),
            },
            AgentEvent::UserPromptAccepted {
                provider_message_id,
                text,
                // Claude leaves the neutral `at_ms` unset (its `created_at` comes
                // from the transcript fold), so it stays out of the golden shape
                // and the projection remains byte-identical.
                at_ms: _,
            } => GoldenEvent::UserPromptAccepted {
                provider_message_id: provider_message_id.clone(),
                text: text.clone(),
            },
            AgentEvent::AssistantDelta {
                provider_item_id,
                text,
            } => GoldenEvent::AssistantDelta {
                provider_item_id: provider_item_id.clone(),
                text: text.clone(),
            },
            AgentEvent::AssistantMessage {
                provider_item_id,
                text,
                at_ms: _,
            } => GoldenEvent::AssistantMessage {
                provider_item_id: provider_item_id.clone(),
                text: text.clone(),
            },
            AgentEvent::ThinkingDelta {
                provider_item_id,
                text,
            } => GoldenEvent::ThinkingDelta {
                provider_item_id: provider_item_id.clone(),
                text: text.clone(),
            },
            AgentEvent::ThinkingMessage {
                provider_item_id,
                text,
                at_ms: _,
            } => GoldenEvent::ThinkingMessage {
                provider_item_id: provider_item_id.clone(),
                text: text.clone(),
            },
            AgentEvent::ToolStarted {
                provider_item_id,
                name,
                input_json,
                at_ms: _,
            } => GoldenEvent::ToolStarted {
                provider_item_id: provider_item_id.clone(),
                name: name.clone(),
                input_json: input_json.clone(),
            },
            AgentEvent::ToolCompleted {
                provider_item_id,
                output_json,
                at_ms: _,
            } => GoldenEvent::ToolCompleted {
                provider_item_id: provider_item_id.clone(),
                output_json: output_json.clone(),
            },
            AgentEvent::PermissionRequested { request } => GoldenEvent::PermissionRequested {
                request_id: request.request_id.clone(),
                tool_name: request.tool_name.clone(),
                input_json: request.input_json.clone(),
                tool_use_id: request.tool_use_id.clone(),
            },
            AgentEvent::PermissionResolved {
                request_id,
                decision,
            } => GoldenEvent::PermissionResolved {
                request_id: request_id.clone(),
                decision: match decision {
                    PermissionDecision::Allow => "allow",
                    PermissionDecision::Deny => "deny",
                },
            },
            AgentEvent::UnsupportedInteraction {
                method,
                detail_json,
            } => GoldenEvent::UnsupportedInteraction {
                method: method.clone(),
                detail_json: detail_json.clone(),
            },
            AgentEvent::TokenUsageUpdated { usage } => GoldenEvent::TokenUsageUpdated {
                context_used_percentage: usage.context_used_percentage,
                context_window_size: usage.context_window_size,
                context_current_usage: usage.context_current_usage,
                total_input_tokens: usage.total_input_tokens,
            },
            AgentEvent::RateLimitsUpdated { windows } => GoldenEvent::RateLimitsUpdated {
                windows: windows
                    .iter()
                    .map(|window| GoldenRateLimitWindow {
                        duration_seconds: window.duration_seconds,
                        used_percentage: window.used_percentage,
                        resets_at: window.resets_at,
                    })
                    .collect(),
            },
            AgentEvent::TurnCompleted { status } => GoldenEvent::TurnCompleted {
                status: match status {
                    TurnStatus::Completed => "completed",
                    TurnStatus::Interrupted => "interrupted",
                    TurnStatus::Failed => "failed",
                },
            },
            AgentEvent::Error {
                recoverable,
                message,
            } => GoldenEvent::Error {
                recoverable: *recoverable,
                message: message.clone(),
            },
        }
    }
}

/// A scenario's name and the event sequence it projects, as checked in.
#[derive(Serialize)]
struct GoldenScenario {
    name: &'static str,
    events: Vec<GoldenEvent>,
}

// --- Runner -----------------------------------------------------------------

fn launch_request() -> LaunchRequest {
    LaunchRequest {
        session_id: SESSION_ID.to_owned(),
        workdir: "/tmp/workdir".to_owned(),
        launch_options: Vec::new(),
        first_prompt: None,
    }
}

/// Drain every buffered event. The adapter must already be dropped so the
/// channel is closed and this returns rather than blocks.
async fn drain(stream: &mut AgentEventStream) -> Vec<AgentEvent> {
    let mut events = Vec::new();
    while let Some(event) = stream.recv().await {
        events.push(event);
    }
    events
}

/// Launch a fresh adapter, feed the scenario's wire inputs through the
/// ingestion seam, and return the full projected event stream.
async fn run(scenario: &Scenario) -> Vec<AgentEvent> {
    let adapter = adapter();
    let handle = adapter.launch(launch_request()).await.expect("launch");
    let mut stream = adapter.events(&handle);
    for step in scenario.steps {
        match step {
            Step::Hook(payload) => adapter.ingest_hook(&handle, payload).unwrap_or_else(|e| {
                panic!("scenario {}: modeled hook rejected: {e}", scenario.name)
            }),
            Step::UnmodeledHook(payload) => {
                let rejected = adapter.ingest_hook(&handle, payload);
                assert!(
                    rejected.is_err(),
                    "scenario {}: an unmodeled hook must be rejected, not projected",
                    scenario.name
                );
            }
            Step::Transcript(lines) => {
                let owned: Vec<String> = lines.iter().map(|l| (*l).to_owned()).collect();
                adapter.ingest_transcript_lines(&handle, &owned);
            }
        }
    }
    drop(adapter);
    drain(&mut stream).await
}

fn golden_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/hook_transcript_mapping.json")
}

#[tokio::test]
async fn hook_transcript_mapping_matches_golden() {
    let mut projected = Vec::new();
    for scenario in scenarios() {
        let events = run(&scenario).await;
        projected.push(GoldenScenario {
            name: scenario.name,
            events: events.iter().map(GoldenEvent::from).collect(),
        });
    }
    let actual = serde_json::to_string_pretty(&projected).expect("serialize golden") + "\n";

    let path = golden_path();
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, &actual)
            .unwrap_or_else(|e| panic!("write golden {}: {e}", path.display()));
        return;
    }

    let expected = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "read golden {} ({e}); run with UPDATE_GOLDEN=1 to create it",
            path.display()
        )
    });
    if expected == actual {
        return;
    }
    panic!(
        "hook/transcript → AgentEvent projection diverges from {}\n{}\n\
         (run with UPDATE_GOLDEN=1 to bless the new output, then review the diff)",
        path.display(),
        line_diff(&expected, &actual),
    );
}

/// A minimal line diff of two pretty-printed JSON documents: every differing
/// line prefixed `-` (expected) / `+` (actual). Mirrors the attribution
/// corpus's diff so a divergence reads as a focused patch, not two dumped blobs.
fn line_diff(expected: &str, actual: &str) -> String {
    let exp: Vec<&str> = expected.lines().collect();
    let act: Vec<&str> = actual.lines().collect();
    let mut out = String::new();
    for i in 0..exp.len().max(act.len()) {
        let e = exp.get(i).copied();
        let a = act.get(i).copied();
        if e != a {
            if let Some(e) = e {
                out.push_str(&format!("  - {e}\n"));
            }
            if let Some(a) = a {
                out.push_str(&format!("  + {a}\n"));
            }
        }
    }
    out
}
