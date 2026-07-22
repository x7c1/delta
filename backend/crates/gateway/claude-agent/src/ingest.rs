//! The adapter's ingestion seam: feeding Claude's lossy wire input into the
//! provider-neutral [`AgentEvent`] projection.
//!
//! Claude Code does not push a structured event stream; its activity is
//! observed through HTTP hooks and the JSONL transcript tail. This module holds
//! the gateway-side wire models and the parsing that turns those raw signals
//! into [`AgentEvent`]s, so the core never sees a hook payload or a transcript
//! line — only neutral facts.
//!
//! ## Scope in this phase
//!
//! The **permission** and **turn-lifecycle** projections are wired:
//!
//! - The `PermissionRequest` hook becomes [`AgentEvent::PermissionRequested`],
//!   and the correlated `tool_result` transcript line becomes
//!   [`AgentEvent::PermissionResolved`].
//! - The `UserPromptSubmit` hook (Claude's prompt echo) becomes
//!   [`AgentEvent::TurnStarted`] — the turn-start half of
//!   `send_emits_user_prompt_and_turn_started`, whose `UserPromptAccepted` half
//!   is emitted by the adapter's `send` (the mechanical dispatch fact).
//! - The `Stop` hook becomes [`AgentEvent::TurnCompleted`] with
//!   [`TurnStatus::Completed`].
//! - The `[Request interrupted by user…]` transcript marker becomes
//!   [`AgentEvent::TurnCompleted`] with [`TurnStatus::Interrupted`] — a turn
//!   that ended without a `Stop` hook.
//!
//! The seam is shaped so the remaining projections (tool blocks →
//! `ToolStarted`/`ToolCompleted`, assistant text → `AssistantMessage`) slot in
//! as new [`ClaudeHook`] variants and transcript-line matchers without changing
//! the adapter's public surface. The transcript matcher here is deliberately
//! minimal; a later phase replaces its internals with the shared transcript
//! reader while keeping this projection's meaning.
//!
//! ## Correlation stays here
//!
//! Claude's `PermissionRequest` hook carries no id of its own, so the adapter
//! mints the request id it emits and remembers the single open dialog per
//! session (Claude shows one at a time). The resolving `tool_result` is
//! correlated back to that open dialog — by `tool_use_id` when the transcript
//! line carries one, otherwise by "the one dialog that is open". This
//! projection-owned correlation is exactly what keeps the emitted [`AgentEvent`]
//! stream clean.

use serde::Deserialize;
use serde_json::Value;

use delta_usecase::{AgentEvent, AgentPermissionRequest, PermissionDecision, TurnStatus};

/// Prefix Claude Code writes as a `role: user` transcript line when the user
/// interrupts the in-flight turn (`[Request interrupted by user]` or
/// `[Request interrupted by user for tool use]`). Matching on the shared prefix
/// covers both variants. This is the gateway-local copy of the same textual
/// convention the transcript reader recognises; a later phase folds this
/// projection into the shared reader.
const INTERRUPT_MARKER_PREFIX: &str = "[Request interrupted by user";

/// A parsed Claude hook payload the adapter projects onto its event stream.
///
/// Extensible by design: this phase models the permission-request, prompt-echo,
/// and stop hooks; the tool hooks join as further variants. The `hook` tag
/// selects the variant so a single [`ingest_hook`] entry point can grow.
///
/// [`ingest_hook`]: crate::ClaudeCodePtyHookAdapter::ingest_hook
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "hook", rename_all = "snake_case")]
pub enum ClaudeHook {
    /// The `PermissionRequest` hook: an interactive permission dialog appeared
    /// for a gated tool call. The payload names the tool and its input; it
    /// carries no id (Delta/the adapter mint the correlation id), and may carry
    /// a `tool_use_id` when the gated call exposes one.
    PermissionRequest {
        tool_name: String,
        #[serde(default)]
        tool_input: Value,
        #[serde(default)]
        tool_use_id: Option<String>,
    },
    /// The `UserPromptSubmit` hook: Claude accepted a prompt into a turn (the
    /// echo of Delta's dispatch, or a prompt typed straight into the pane). It
    /// is the turn-start signal — projected as [`AgentEvent::TurnStarted`]. The
    /// prompt text is not read here: the accepted-prompt fact
    /// ([`AgentEvent::UserPromptAccepted`]) is emitted by the adapter's `send`;
    /// this hook contributes only the turn boundary.
    UserPromptSubmit {},
    /// The `Stop` hook: the in-flight turn completed. Projected as
    /// [`AgentEvent::TurnCompleted`] with [`TurnStatus::Completed`].
    Stop {},
}

/// A permission dialog the adapter has projected and not yet resolved.
///
/// Claude shows one permission dialog at a time, so a session holds at most one
/// of these. It carries the minted `request_id` the emitted events are keyed by
/// and the `tool_use_id` (when known) used to correlate the resolving
/// `tool_result`.
#[derive(Debug, Clone)]
pub(crate) struct OpenPermission {
    pub request_id: String,
    pub tool_use_id: Option<String>,
}

/// The minimal `tool_result` view the permission projection needs.
///
/// Deliberately lenient: a transcript line that is not a JSON `tool_result`
/// simply fails to match and is skipped, so feeding a whole transcript tail is
/// safe. `is_error` is Claude's own flag on a tool result; a denied permission
/// surfaces as an errored result, so this phase reads it as the deny signal.
#[derive(Debug, Clone, Deserialize)]
struct ToolResultLine {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    tool_use_id: Option<String>,
    #[serde(default)]
    is_error: bool,
}

/// Parse a transcript line as a permission-resolving `tool_result`, or `None`
/// when the line is anything else (blank, non-JSON, or a different record).
fn tool_result_from_line(line: &str) -> Option<ToolResultLine> {
    let parsed: ToolResultLine = serde_json::from_str(line).ok()?;
    (parsed.kind == "tool_result").then_some(parsed)
}

/// Project a parsed [`ClaudeHook`] into the event it raises and the open-dialog
/// state it establishes, if any.
///
/// Pure so the projection is unit-testable without the adapter's channels.
/// `request_id` is minted by the caller (the adapter's per-session sequence)
/// and used only by the permission-request variant, which is the one hook whose
/// event is keyed by an id Claude's payload does not carry; the turn hooks
/// ignore it.
pub(crate) fn project_hook(
    hook: ClaudeHook,
    request_id: String,
) -> (AgentEvent, Option<OpenPermission>) {
    match hook {
        ClaudeHook::PermissionRequest {
            tool_name,
            tool_input,
            tool_use_id,
        } => {
            let open = OpenPermission {
                request_id: request_id.clone(),
                tool_use_id: tool_use_id.clone(),
            };
            let event = AgentEvent::PermissionRequested {
                request: AgentPermissionRequest {
                    request_id,
                    tool_name,
                    input_json: tool_input,
                    tool_use_id,
                },
            };
            (event, Some(open))
        }
        ClaudeHook::UserPromptSubmit {} => (
            // Claude confirms a turn via the prompt echo, without naming it.
            AgentEvent::TurnStarted {
                provider_turn_id: None,
            },
            None,
        ),
        ClaudeHook::Stop {} => (
            AgentEvent::TurnCompleted {
                status: TurnStatus::Completed,
            },
            None,
        ),
    }
}

/// Whether a hook mints a permission correlation id. Only the permission
/// request does (Claude's payload carries none); the turn hooks are keyed by no
/// id, so the caller must not burn a sequence number on them.
pub(crate) fn hook_needs_request_id(hook: &ClaudeHook) -> bool {
    matches!(hook, ClaudeHook::PermissionRequest { .. })
}

/// The minimal `role: user` transcript-line view the interrupt-marker
/// projection needs. Deliberately lenient, like [`ToolResultLine`]: any line
/// that is not a JSON user record with text simply fails to match.
#[derive(Debug, Clone, Deserialize)]
struct UserLine {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: Option<String>,
}

/// Project a transcript line as the turn-ending interrupt marker, or `None`
/// when the line is anything else.
///
/// Claude writes a `role: user` line whose text is `[Request interrupted by
/// user…]` when the user aborts the in-flight turn; no `Stop` hook fires, so
/// this marker is the turn-end signal. It projects
/// [`AgentEvent::TurnCompleted`] with [`TurnStatus::Interrupted`]. Carries no
/// state, so it is safe to try against every transcript line.
pub(crate) fn project_interrupt_marker(line: &str) -> Option<AgentEvent> {
    let parsed: UserLine = serde_json::from_str(line).ok()?;
    let text = parsed.text?;
    (parsed.kind == "user" && text.trim_start().starts_with(INTERRUPT_MARKER_PREFIX)).then_some(
        AgentEvent::TurnCompleted {
            status: TurnStatus::Interrupted,
        },
    )
}

/// Project a transcript line against the session's open permission dialog.
///
/// Returns the [`AgentEvent::PermissionResolved`] to emit when the line is the
/// `tool_result` that resolves the open dialog, or `None` when the line is
/// unrelated (not a tool result, or a tool result for a different call while a
/// `tool_use_id` is known on both sides). A match consumes the open dialog (the
/// caller clears it), mirroring Delta's one-dialog-at-a-time runtime model.
pub(crate) fn project_transcript_line(line: &str, open: &OpenPermission) -> Option<AgentEvent> {
    let result = tool_result_from_line(line)?;
    // Correlate to the open dialog: match when either side lacks a
    // `tool_use_id` (nothing to disambiguate — it is the one open dialog) or
    // both carry the same one.
    let correlated = match (&result.tool_use_id, &open.tool_use_id) {
        (Some(from_line), Some(from_open)) => from_line == from_open,
        _ => true,
    };
    if !correlated {
        return None;
    }
    let decision = if result.is_error {
        PermissionDecision::Deny
    } else {
        PermissionDecision::Allow
    };
    Some(AgentEvent::PermissionResolved {
        request_id: open.request_id.clone(),
        decision,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(request_id: &str, tool_use_id: Option<&str>) -> OpenPermission {
        OpenPermission {
            request_id: request_id.to_owned(),
            tool_use_id: tool_use_id.map(str::to_owned),
        }
    }

    #[test]
    fn a_permission_request_hook_projects_the_requested_event() {
        let hook: ClaudeHook = serde_json::from_str(
            r#"{"hook":"permission_request","tool_name":"Bash",
                "tool_input":{"command":"ls"},"tool_use_id":"tu-1"}"#,
        )
        .expect("parse");
        let (event, open) = project_hook(hook, "req-1".to_owned());

        match event {
            AgentEvent::PermissionRequested { request } => {
                assert_eq!(request.request_id, "req-1");
                assert_eq!(request.tool_name, "Bash");
                assert_eq!(request.input_json, serde_json::json!({ "command": "ls" }));
                assert_eq!(request.tool_use_id.as_deref(), Some("tu-1"));
            }
            other => panic!("expected PermissionRequested, got {other:?}"),
        }
        let open = open.expect("an open dialog is tracked");
        assert_eq!(open.request_id, "req-1");
        assert_eq!(open.tool_use_id.as_deref(), Some("tu-1"));
    }

    #[test]
    fn a_correlated_tool_result_resolves_the_open_dialog() {
        let open = open("req-1", Some("tu-1"));
        let allowed =
            project_transcript_line(r#"{"type":"tool_result","tool_use_id":"tu-1"}"#, &open);
        assert!(matches!(
            allowed,
            Some(AgentEvent::PermissionResolved {
                request_id,
                decision: PermissionDecision::Allow,
            }) if request_id == "req-1"
        ));

        let denied = project_transcript_line(
            r#"{"type":"tool_result","tool_use_id":"tu-1","is_error":true}"#,
            &open,
        );
        assert!(matches!(
            denied,
            Some(AgentEvent::PermissionResolved {
                decision: PermissionDecision::Deny,
                ..
            })
        ));
    }

    #[test]
    fn a_tool_result_for_a_different_call_does_not_resolve() {
        let open = open("req-1", Some("tu-1"));
        let event =
            project_transcript_line(r#"{"type":"tool_result","tool_use_id":"tu-other"}"#, &open);
        assert!(
            event.is_none(),
            "a tool_result for a different tool_use_id must not resolve this dialog"
        );
    }

    #[test]
    fn a_tool_result_without_a_tool_use_id_resolves_the_one_open_dialog() {
        // Claude's `PermissionRequest` carries no `tool_use_id`, so the open
        // dialog may have none; the single open dialog is resolved by the next
        // tool_result regardless.
        let open = open("req-1", None);
        let event = project_transcript_line(r#"{"type":"tool_result"}"#, &open);
        assert!(matches!(event, Some(AgentEvent::PermissionResolved { .. })));
    }

    #[test]
    fn non_tool_result_lines_are_skipped() {
        let open = open("req-1", Some("tu-1"));
        assert!(project_transcript_line("", &open).is_none());
        assert!(project_transcript_line("not json", &open).is_none());
        assert!(project_transcript_line(r#"{"type":"assistant"}"#, &open).is_none());
    }

    #[test]
    fn a_user_prompt_submit_hook_projects_turn_started() {
        let hook: ClaudeHook =
            serde_json::from_str(r#"{"hook":"user_prompt_submit","prompt":"hello"}"#)
                .expect("parse");
        let (event, open) = project_hook(hook, String::new());
        assert!(matches!(
            event,
            AgentEvent::TurnStarted {
                provider_turn_id: None
            }
        ));
        assert!(
            open.is_none(),
            "a prompt echo opens no permission dialog to track"
        );
    }

    #[test]
    fn a_stop_hook_projects_turn_completed() {
        let hook: ClaudeHook = serde_json::from_str(r#"{"hook":"stop"}"#).expect("parse");
        let (event, open) = project_hook(hook, String::new());
        assert!(matches!(
            event,
            AgentEvent::TurnCompleted {
                status: TurnStatus::Completed
            }
        ));
        assert!(open.is_none());
    }

    #[test]
    fn only_the_permission_request_hook_mints_a_request_id() {
        let permission: ClaudeHook =
            serde_json::from_str(r#"{"hook":"permission_request","tool_name":"Bash"}"#)
                .expect("parse");
        assert!(hook_needs_request_id(&permission));
        let prompt: ClaudeHook =
            serde_json::from_str(r#"{"hook":"user_prompt_submit"}"#).expect("parse");
        assert!(!hook_needs_request_id(&prompt));
        let stop: ClaudeHook = serde_json::from_str(r#"{"hook":"stop"}"#).expect("parse");
        assert!(!hook_needs_request_id(&stop));
    }

    #[test]
    fn both_interrupt_marker_variants_project_an_interrupted_completion() {
        for text in [
            "[Request interrupted by user]",
            "[Request interrupted by user for tool use]",
        ] {
            let line = serde_json::json!({ "type": "user", "text": text }).to_string();
            assert!(
                matches!(
                    project_interrupt_marker(&line),
                    Some(AgentEvent::TurnCompleted {
                        status: TurnStatus::Interrupted
                    })
                ),
                "expected an interrupted completion for {text:?}"
            );
        }
    }

    #[test]
    fn non_interrupt_lines_do_not_project_a_completion() {
        assert!(project_interrupt_marker("").is_none());
        assert!(project_interrupt_marker("not json").is_none());
        // A normal user prompt is not the interrupt marker.
        assert!(project_interrupt_marker(r#"{"type":"user","text":"a normal prompt"}"#).is_none());
        // The marker text on a non-user line does not count.
        assert!(project_interrupt_marker(
            r#"{"type":"assistant","text":"[Request interrupted by user]"}"#
        )
        .is_none());
    }
}
