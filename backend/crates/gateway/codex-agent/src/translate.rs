//! Translating `codex app-server` wire frames into Delta's neutral
//! [`AgentEvent`]s.
//!
//! This is the heart of the Codex adapter: the app-server pushes structured
//! `turn/*` / `item/*` notifications and `*/requestApproval` server → client
//! requests, and this module turns each into the provider-neutral facts the
//! core reasons over. It is a pure function of the wire frame (no I/O), so the
//! mapping is unit-tested in isolation.
//!
//! The `turn/*` envelope is reconciled against the vendored v2 schema: both
//! `turn/started` and `turn/completed` wrap a `Turn` object under `params.turn`,
//! so the turn id is `params.turn.id` and the terminal status `params.turn.status`
//! (one of `completed` / `interrupted` / `failed` / `inProgress`).
//!
//! **Still inferred, not yet reconciled (R2/R3):** the `item.itemType` vocabulary
//! (`agent_message` vs. tool items), the rich item-content notifications
//! (`item/agentMessage/delta`, `item/reasoning/*`, …), and the fields carried on
//! an approval request (`itemId`, `toolName`) plus the approval method fan-out.
//! The translation is deliberately lenient — an unknown notification maps to
//! nothing rather than erroring, and an unknown item type is treated as a tool —
//! so those later corrections stay localised to this file and the `wire` module.

use serde_json::Value;

use delta_usecase::{AgentEvent, AgentPermissionRequest, TurnStatus};

use crate::wire::{Notification, ServerRequest};

/// The item type (`item.itemType`) that carries an assistant message. Every
/// other item type is treated as a tool call. Matched case- and
/// separator-insensitively so both `agent_message` (the fake's shape) and a
/// real server's `agentMessage` resolve to the same thing.
const AGENT_MESSAGE_ITEM_TYPE: &str = "agentmessage";

/// The classification of a server-originated request.
#[derive(Debug, Clone, PartialEq)]
pub enum ServerRequestKind {
    /// A permission/approval request Delta models: it becomes a
    /// [`AgentEvent::PermissionRequested`] and is answered with a decision.
    Approval(AgentPermissionRequest),
    /// A server → client request Delta does not model. It must be surfaced as
    /// [`AgentEvent::UnsupportedInteraction`] and answered (with an error) so
    /// the session never silently hangs on it.
    Unsupported { method: String, detail_json: Value },
}

/// Translate a thread-scoped notification into zero or more neutral events.
///
/// A notification this build does not model yields an empty vector rather than
/// an error, so a newer server emitting extra notifications degrades to silence
/// instead of tearing the session down.
pub fn translate_notification(n: &Notification) -> Vec<AgentEvent> {
    match n.method.as_str() {
        "turn/started" => vec![AgentEvent::TurnStarted {
            provider_turn_id: notification_turn_id(&n.params),
        }],
        "turn/completed" => vec![AgentEvent::TurnCompleted {
            status: turn_status(notification_turn_status(&n.params).as_deref()),
        }],
        "item/started" => item_event(item_of(&n.params), true),
        "item/completed" => item_event(item_of(&n.params), false),
        _ => Vec::new(),
    }
}

/// The id of the turn a `turn/started` notification announces — the id
/// `turn/interrupt` must reference — for the adapter's per-session turn tracking.
/// `None` for any other notification (or a `turn/started` missing its id).
pub fn started_turn_id(n: &Notification) -> Option<String> {
    match n.method.as_str() {
        "turn/started" => notification_turn_id(&n.params),
        _ => None,
    }
}

/// Whether a notification is the `turn/completed` that ends the current turn, so
/// the adapter can clear its tracked turn id when the turn finishes.
pub fn is_turn_completed(n: &Notification) -> bool {
    n.method == "turn/completed"
}

/// Classify a server-originated request as a modeled approval or an unmodeled
/// interaction.
pub fn classify_server_request(r: &ServerRequest) -> ServerRequestKind {
    if is_approval_method(&r.method) {
        ServerRequestKind::Approval(approval_request(r))
    } else {
        ServerRequestKind::Unsupported {
            method: r.method.clone(),
            detail_json: r.params.clone(),
        }
    }
}

/// The neutral permission request an approval server-request projects to. The
/// `request_id` is the server request id rendered as a string — the same value
/// the adapter maps back to the verbatim wire id when it answers.
fn approval_request(r: &ServerRequest) -> AgentPermissionRequest {
    AgentPermissionRequest {
        request_id: request_id_of(&r.id),
        tool_name: string_field(&r.params, "toolName").unwrap_or_default(),
        input_json: r.params.clone(),
        tool_use_id: string_field(&r.params, "itemId"),
    }
}

/// Render a server request id as the neutral, stringly-typed request id. A
/// string id is used as-is; any other JSON id is rendered canonically so it
/// still round-trips to a lookup key.
pub fn request_id_of(id: &Value) -> String {
    match id {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Whether a server → client method is an approval request Delta models. The
/// frozen contract names these `*/requestApproval`.
fn is_approval_method(method: &str) -> bool {
    method.ends_with("requestApproval")
}

/// The `item` object a notification carries under `params.item`, if any.
fn item_of(params: &Value) -> Option<&Value> {
    params.get("item")
}

/// Project an `item/started` or `item/completed` into its neutral event(s).
///
/// An assistant-message item becomes an [`AgentEvent::AssistantDelta`] while it
/// is streaming (a non-empty `started` fragment) and an
/// [`AgentEvent::AssistantMessage`] once completed. Every other item type is a
/// tool call: [`AgentEvent::ToolStarted`] then [`AgentEvent::ToolCompleted`].
fn item_event(item: Option<&Value>, started: bool) -> Vec<AgentEvent> {
    let Some(item) = item else {
        return Vec::new();
    };
    let provider_item_id = string_field(item, "id").unwrap_or_default();
    if is_agent_message(item) {
        let text = string_field(item, "text").unwrap_or_default();
        if started {
            // A started assistant item with no text yet is just "the assistant
            // is about to speak" — nothing to show, so emit nothing rather than
            // an empty delta.
            if text.is_empty() {
                Vec::new()
            } else {
                vec![AgentEvent::AssistantDelta {
                    provider_item_id,
                    text,
                }]
            }
        } else {
            vec![AgentEvent::AssistantMessage {
                provider_item_id,
                text,
            }]
        }
    } else if started {
        vec![AgentEvent::ToolStarted {
            provider_item_id,
            name: tool_name(item),
            input_json: item.get("input").cloned().unwrap_or_else(|| item.clone()),
        }]
    } else {
        vec![AgentEvent::ToolCompleted {
            provider_item_id,
            output_json: item.get("output").cloned().unwrap_or_else(|| item.clone()),
        }]
    }
}

/// Whether an item is an assistant message (as opposed to a tool call).
fn is_agent_message(item: &Value) -> bool {
    string_field(item, "itemType")
        .map(|t| normalise_item_type(&t) == AGENT_MESSAGE_ITEM_TYPE)
        .unwrap_or(false)
}

/// The tool name for a tool item: its explicit `toolName`/`name` when present,
/// otherwise the raw `itemType` (so a `command_execution` item at least names
/// its kind).
fn tool_name(item: &Value) -> String {
    string_field(item, "toolName")
        .or_else(|| string_field(item, "name"))
        .or_else(|| string_field(item, "itemType"))
        .unwrap_or_default()
}

/// Lowercase and drop `_`/`-` so `agent_message`, `agentMessage`, and
/// `agent-message` compare equal.
fn normalise_item_type(item_type: &str) -> String {
    item_type
        .chars()
        .filter(|c| *c != '_' && *c != '-')
        .flat_map(char::to_lowercase)
        .collect()
}

/// The turn id a `turn/*` notification carries under `params.turn.id`. Both
/// `turn/started` and `turn/completed` wrap the turn in a `Turn` object (see the
/// vendored `TurnStartedNotification` / `TurnCompletedNotification` schemas).
fn notification_turn_id(params: &Value) -> Option<String> {
    turn_field(params, "id")
}

/// The terminal status a `turn/completed` notification carries under
/// `params.turn.status`.
fn notification_turn_status(params: &Value) -> Option<String> {
    turn_field(params, "status")
}

/// Read a string field from the `turn` object a `turn/*` notification wraps.
fn turn_field(params: &Value, key: &str) -> Option<String> {
    params.get("turn").and_then(|turn| string_field(turn, key))
}

/// Map a `turn/completed` status string to the neutral [`TurnStatus`]. An
/// absent or unrecognised status is treated as [`TurnStatus::Failed`]: a turn
/// that ended in a shape we cannot read is not assumed to have succeeded.
fn turn_status(status: Option<&str>) -> TurnStatus {
    match status {
        Some("completed") => TurnStatus::Completed,
        Some("interrupted") => TurnStatus::Interrupted,
        _ => TurnStatus::Failed,
    }
}

/// Read a string field from a JSON object, returning `None` when absent or not
/// a string.
fn string_field(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn notification(method: &str, params: Value) -> Notification {
        Notification {
            method: method.to_owned(),
            params,
        }
    }

    #[test]
    fn turn_started_carries_the_turn_id_from_the_nested_turn() {
        let events = translate_notification(&notification(
            "turn/started",
            json!({ "threadId": "thr_1", "turn": { "id": "turn_1", "status": "inProgress", "items": [] } }),
        ));
        assert_eq!(
            events,
            vec![AgentEvent::TurnStarted {
                provider_turn_id: Some("turn_1".to_owned())
            }]
        );
    }

    #[test]
    fn turn_started_without_a_turn_id_is_still_a_turn() {
        let events = translate_notification(&notification(
            "turn/started",
            json!({ "threadId": "thr_1" }),
        ));
        assert_eq!(
            events,
            vec![AgentEvent::TurnStarted {
                provider_turn_id: None
            }]
        );
    }

    #[test]
    fn turn_completed_maps_each_status_from_the_nested_turn() {
        for (wire, expected) in [
            ("completed", TurnStatus::Completed),
            ("interrupted", TurnStatus::Interrupted),
            ("failed", TurnStatus::Failed),
        ] {
            let events = translate_notification(&notification(
                "turn/completed",
                json!({ "threadId": "thr_1", "turn": { "id": "turn_1", "status": wire, "items": [] } }),
            ));
            assert_eq!(events, vec![AgentEvent::TurnCompleted { status: expected }]);
        }
    }

    #[test]
    fn an_unknown_or_absent_status_is_failed() {
        assert_eq!(
            translate_notification(&notification(
                "turn/completed",
                json!({ "turn": { "id": "turn_1", "status": "weird", "items": [] } })
            )),
            vec![AgentEvent::TurnCompleted {
                status: TurnStatus::Failed
            }]
        );
        assert_eq!(
            translate_notification(&notification("turn/completed", json!({}))),
            vec![AgentEvent::TurnCompleted {
                status: TurnStatus::Failed
            }]
        );
    }

    #[test]
    fn started_turn_id_reads_the_nested_turn_id_only_for_turn_started() {
        assert_eq!(
            started_turn_id(&notification(
                "turn/started",
                json!({ "threadId": "t", "turn": { "id": "turn_7", "status": "inProgress", "items": [] } })
            )),
            Some("turn_7".to_owned())
        );
        // A `turn/completed` is not where the adapter learns the active turn id.
        assert_eq!(
            started_turn_id(&notification(
                "turn/completed",
                json!({ "turn": { "id": "turn_7", "status": "completed", "items": [] } })
            )),
            None
        );
        assert!(is_turn_completed(&notification(
            "turn/completed",
            json!({})
        )));
        assert!(!is_turn_completed(&notification("turn/started", json!({}))));
    }

    #[test]
    fn agent_message_completed_is_an_assistant_message() {
        let events = translate_notification(&notification(
            "item/completed",
            json!({ "item": { "id": "item_1", "itemType": "agent_message", "text": "hi" } }),
        ));
        assert_eq!(
            events,
            vec![AgentEvent::AssistantMessage {
                provider_item_id: "item_1".to_owned(),
                text: "hi".to_owned(),
            }]
        );
    }

    #[test]
    fn agent_message_started_with_text_is_a_delta_and_empty_is_nothing() {
        let with_text = translate_notification(&notification(
            "item/started",
            json!({ "item": { "id": "i1", "itemType": "agentMessage", "text": "partial" } }),
        ));
        assert_eq!(
            with_text,
            vec![AgentEvent::AssistantDelta {
                provider_item_id: "i1".to_owned(),
                text: "partial".to_owned(),
            }]
        );

        let empty = translate_notification(&notification(
            "item/started",
            json!({ "item": { "id": "i1", "itemType": "agent_message" } }),
        ));
        assert!(empty.is_empty(), "an empty started message emits nothing");
    }

    #[test]
    fn a_non_message_item_is_a_tool_call() {
        let started = translate_notification(&notification(
            "item/started",
            json!({ "item": { "id": "t1", "itemType": "command_execution", "input": { "command": "ls" } } }),
        ));
        assert_eq!(
            started,
            vec![AgentEvent::ToolStarted {
                provider_item_id: "t1".to_owned(),
                name: "command_execution".to_owned(),
                input_json: json!({ "command": "ls" }),
            }]
        );

        let completed = translate_notification(&notification(
            "item/completed",
            json!({ "item": { "id": "t1", "itemType": "command_execution", "output": { "exitCode": 0 } } }),
        ));
        assert_eq!(
            completed,
            vec![AgentEvent::ToolCompleted {
                provider_item_id: "t1".to_owned(),
                output_json: json!({ "exitCode": 0 }),
            }]
        );
    }

    #[test]
    fn an_unmodeled_notification_translates_to_nothing() {
        assert!(translate_notification(&notification(
            "thread/somethingNew",
            json!({ "threadId": "thr_1" })
        ))
        .is_empty());
    }

    #[test]
    fn an_approval_request_becomes_a_permission_request() {
        let request = ServerRequest {
            id: json!("srv-1"),
            method: "item/requestApproval".to_owned(),
            params: json!({ "threadId": "thr_1", "itemId": "item_9", "toolName": "Bash" }),
        };
        match classify_server_request(&request) {
            ServerRequestKind::Approval(req) => {
                assert_eq!(req.request_id, "srv-1");
                assert_eq!(req.tool_name, "Bash");
                assert_eq!(req.tool_use_id, Some("item_9".to_owned()));
            }
            other => panic!("expected an approval, got {other:?}"),
        }
    }

    #[test]
    fn any_other_request_approval_method_is_still_modeled() {
        let request = ServerRequest {
            id: json!(42),
            method: "turn/requestApproval".to_owned(),
            params: json!({}),
        };
        match classify_server_request(&request) {
            // A non-string id renders canonically so it still keys a lookup.
            ServerRequestKind::Approval(req) => assert_eq!(req.request_id, "42"),
            other => panic!("expected an approval, got {other:?}"),
        }
    }

    #[test]
    fn an_unmodeled_server_request_is_unsupported() {
        let request = ServerRequest {
            id: json!("srv-2"),
            method: "session/requestUserInput".to_owned(),
            params: json!({ "threadId": "thr_1", "prompt": "pick one" }),
        };
        match classify_server_request(&request) {
            ServerRequestKind::Unsupported {
                method,
                detail_json,
            } => {
                assert_eq!(method, "session/requestUserInput");
                assert_eq!(detail_json["prompt"], "pick one");
            }
            other => panic!("expected unsupported, got {other:?}"),
        }
    }
}
