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
//! The approval fan-out is reconciled against the vendored `ServerRequest`
//! registry (see `vendor/app-server-schema/`): the server drives `turn/start`
//! approvals as server → client requests, and Delta models the two whose
//! response is a binary decision —
//! `item/commandExecution/requestApproval` and `item/fileChange/requestApproval`
//! — as [`AgentPermissionRequest`]s built from each method's real params. Every
//! other server → client request — the permissions approval (whose response is a
//! `GrantedPermissionProfile`, not a decision), the experimental tool/user-input
//! and dynamic tool-call requests, MCP elicitation, the deprecated legacy
//! approvals, and anything a newer server adds — is surfaced as
//! [`ServerRequestKind::Unsupported`] so the adapter can answer it and never hang.
//!
//! **Item content (R3, reconciled):** item shapes and the rich item-content
//! notifications are now reconciled against the vendored v2 schema, replacing the
//! earlier infer-itemType / unknown-is-tool heuristic with an explicit match on
//! the real `item.type` vocabulary (`ThreadItem` oneOf: `agentMessage`,
//! `commandExecution`, `fileChange`, `userMessage`, `reasoning`, …) and the real
//! streaming-delta method names (`ServerNotification`: `item/agentMessage/delta`,
//! `item/reasoning/*`, `item/commandExecution/outputDelta`, …). The translation
//! stays deliberately lenient in one direction only — an item type or delta
//! method this build does not model maps to *nothing* (a safe skip), never an
//! error and never a mis-filed tool call.
//!
//! **Reasoning.** [`AgentEvent`] carries a thinking-bearing pair
//! ([`AgentEvent::ThinkingDelta`] / [`AgentEvent::ThinkingMessage`]) distinct
//! from the assistant-reply pair, so the model's internal reasoning is surfaced
//! *as reasoning* — it becomes a `Thinking` content block, exactly like Claude's,
//! and is never folded into reply text. A `reasoning` item and the text-bearing
//! `item/reasoning/*` deltas therefore map onto that pair instead of being
//! dropped.
//!
//! Which reasoning text: the vendored `ReasoningThreadItem` carries two string
//! arrays, `content` (the model's raw reasoning parts) and `summary` (the
//! summarised parts). Delta surfaces `content` when the server provides it and
//! falls back to `summary` otherwise — never both, since the summary is a
//! condensation of the same reasoning and emitting both would show it twice.
//! The fallback is what makes this useful in practice: hosted reasoning models
//! normally withhold raw chain-of-thought and return summaries only, so a
//! `content`-only mapping would yield an empty thinking block on most turns.
//! Parts are joined with a blank line, since each array element is a separate
//! reasoning part. An item with neither maps to nothing, so an empty thinking
//! block is never minted.

use serde_json::Value;

use delta_usecase::{AgentEvent, AgentPermissionRequest, TurnStatus};

use crate::wire::{Notification, ServerRequest};

/// The `item.type` (see the vendored `ThreadItem` oneOf) that carries an
/// assistant message: `AgentMessageThreadItem`, whose `text` is the reply and
/// `id` the provider item id.
const AGENT_MESSAGE_ITEM_TYPE: &str = "agentMessage";
/// The `item.type` for a shell command execution (`CommandExecutionThreadItem`):
/// a tool call carrying `command` / `cwd` / `status` / `aggregatedOutput` /
/// `exitCode`.
const COMMAND_EXECUTION_ITEM_TYPE: &str = "commandExecution";
/// The `item.type` for a file change (`FileChangeThreadItem`): a tool call
/// carrying `changes` / `status`.
const FILE_CHANGE_ITEM_TYPE: &str = "fileChange";
/// The `item.type` for the echoed user prompt (`UserMessageThreadItem`). The
/// visible prompt is already surfaced as [`AgentEvent::UserPromptAccepted`] at
/// send time, so this item is dropped to avoid double-emitting it.
const USER_MESSAGE_ITEM_TYPE: &str = "userMessage";
/// The `item.type` for the model's reasoning (`ReasoningThreadItem`), carrying
/// its `content` / `summary` string arrays. Mapped onto the thinking-bearing
/// events — see the module docs for which of the two fields wins.
const REASONING_ITEM_TYPE: &str = "reasoning";

/// The `ReasoningThreadItem` field holding the model's raw reasoning parts.
/// Preferred over [`REASONING_SUMMARY_FIELD`] when the server provides it.
const REASONING_CONTENT_FIELD: &str = "content";
/// The `ReasoningThreadItem` field holding the summarised reasoning parts. The
/// fallback when the raw `content` is absent — which is the usual case for
/// hosted reasoning models.
const REASONING_SUMMARY_FIELD: &str = "summary";
/// How the parts of a reasoning array are joined into one thinking text. A blank
/// line, because each element is a separate reasoning part (the same boundary
/// `item/reasoning/summaryPartAdded` announces while streaming).
const REASONING_PART_SEPARATOR: &str = "\n\n";

/// The streaming-delta method (`AgentMessageDeltaNotification`) that carries a
/// fragment of an assistant message, under `params.itemId` / `params.delta`.
const METHOD_AGENT_MESSAGE_DELTA: &str = "item/agentMessage/delta";

/// The streaming-delta method (`ReasoningTextDeltaNotification`) carrying a
/// fragment of the model's raw reasoning, under `params.itemId` /
/// `params.delta`.
const METHOD_REASONING_TEXT_DELTA: &str = "item/reasoning/textDelta";
/// The streaming-delta method (`ReasoningSummaryTextDeltaNotification`) carrying
/// a fragment of the model's summarised reasoning, under the same
/// `params.itemId` / `params.delta` shape.
const METHOD_REASONING_SUMMARY_TEXT_DELTA: &str = "item/reasoning/summaryTextDelta";
/// The notification (`ReasoningSummaryPartAddedNotification`) announcing that a
/// new summary part opened. Its params carry only indices — no text — so it
/// projects to nothing; the part boundary it marks is reproduced by
/// [`REASONING_PART_SEPARATOR`] when the completed item is translated.
const METHOD_REASONING_SUMMARY_PART_ADDED: &str = "item/reasoning/summaryPartAdded";

/// The server → client approval request for a command execution (a `turn/start`
/// turn). Response is a binary `{decision}`, so Delta models it.
const METHOD_COMMAND_EXECUTION_APPROVAL: &str = "item/commandExecution/requestApproval";
/// The server → client approval request for a file change (a `turn/start` turn).
/// Response is a binary `{decision}`, so Delta models it.
const METHOD_FILE_CHANGE_APPROVAL: &str = "item/fileChange/requestApproval";

/// Fallback tool name for a command-execution approval whose `command` is absent
/// (the field is nullable in the vendored schema).
const COMMAND_EXECUTION_TOOL_NAME: &str = "command_execution";
/// Tool name for a file-change approval. Its params carry no command or file path
/// (only `itemId` / `grantRoot` / `reason`), so a stable kind label names the
/// interaction while the details ride `input_json`.
const FILE_CHANGE_TOOL_NAME: &str = "file_change";

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
        // `startedAtMs` / `completedAtMs` are siblings of `item` on the
        // notification params (see the vendored `ItemStartedNotification` /
        // `ItemCompletedNotification`), so they are read here and threaded into
        // the projected event as its neutral `at_ms`.
        "item/started" => item_event(
            item_of(&n.params),
            true,
            int_field(&n.params, "startedAtMs"),
        ),
        "item/completed" => item_event(
            item_of(&n.params),
            false,
            int_field(&n.params, "completedAtMs"),
        ),
        METHOD_AGENT_MESSAGE_DELTA => agent_message_delta(&n.params),
        // Both text-bearing reasoning deltas share the `{itemId, delta}` shape
        // and both are fragments of the same item's thinking, so they project to
        // the same neutral fragment.
        METHOD_REASONING_TEXT_DELTA | METHOD_REASONING_SUMMARY_TEXT_DELTA => {
            reasoning_delta(&n.params)
        }
        // Streaming deltas Delta does not model as neutral events are dropped
        // (they still arrive faithfully but project to nothing): the
        // summary-part boundary carries no text of its own, and plan /
        // command-output / MCP progress have no neutral streaming counterpart.
        // Listed explicitly so the intent is a documented skip, not an
        // accidental fall-through.
        METHOD_REASONING_SUMMARY_PART_ADDED
        | "item/plan/delta"
        | "item/commandExecution/outputDelta"
        | "item/mcpToolCall/progress" => Vec::new(),
        _ => Vec::new(),
    }
}

/// Project an `item/agentMessage/delta` notification into a streaming
/// [`AgentEvent::AssistantDelta`]. The real params carry the fragment under
/// `delta` and the item it extends under `itemId` (see
/// `AgentMessageDeltaNotification`). An empty fragment yields nothing.
fn agent_message_delta(params: &Value) -> Vec<AgentEvent> {
    let text = string_field(params, "delta").unwrap_or_default();
    if text.is_empty() {
        return Vec::new();
    }
    vec![AgentEvent::AssistantDelta {
        provider_item_id: string_field(params, "itemId").unwrap_or_default(),
        text,
    }]
}

/// Project an `item/reasoning/textDelta` or `item/reasoning/summaryTextDelta`
/// into a streaming [`AgentEvent::ThinkingDelta`]. Both notifications carry the
/// fragment under `delta` and the item it extends under `itemId` (see the
/// vendored `ReasoningTextDeltaNotification` /
/// `ReasoningSummaryTextDeltaNotification`; they differ only in the
/// `contentIndex` / `summaryIndex` the fragment belongs to, which the neutral
/// event does not model). An empty fragment yields nothing.
fn reasoning_delta(params: &Value) -> Vec<AgentEvent> {
    let text = string_field(params, "delta").unwrap_or_default();
    if text.is_empty() {
        return Vec::new();
    }
    vec![AgentEvent::ThinkingDelta {
        provider_item_id: string_field(params, "itemId").unwrap_or_default(),
        text,
    }]
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
///
/// The fan-out is an explicit allowlist of the two approval methods whose
/// response is a binary decision (`item/commandExecution/requestApproval`,
/// `item/fileChange/requestApproval`) — matched by exact method string, not a
/// `*/requestApproval` suffix heuristic. Everything else, including
/// `item/permissions/requestApproval` (whose response is a
/// `GrantedPermissionProfile`, not a decision Delta can produce), is unsupported:
/// the adapter answers it and surfaces it, so the turn never hangs.
pub fn classify_server_request(r: &ServerRequest) -> ServerRequestKind {
    match r.method.as_str() {
        METHOD_COMMAND_EXECUTION_APPROVAL => {
            ServerRequestKind::Approval(command_execution_approval(r))
        }
        METHOD_FILE_CHANGE_APPROVAL => ServerRequestKind::Approval(file_change_approval(r)),
        _ => ServerRequestKind::Unsupported {
            method: r.method.clone(),
            detail_json: r.params.clone(),
        },
    }
}

/// The neutral permission request a command-execution approval projects to. The
/// command being run (`command`) names the tool — falling back to a stable kind
/// label when the server omits it — `itemId` is the tool-use id, and the full
/// params ride `input_json` so `cwd` / `commandActions` / the proposed amendments
/// are preserved for the UI. The `request_id` is the server request id rendered
/// as a string — the same value the adapter maps back to the verbatim wire id
/// when it answers.
fn command_execution_approval(r: &ServerRequest) -> AgentPermissionRequest {
    AgentPermissionRequest {
        request_id: request_id_of(&r.id),
        tool_name: string_field(&r.params, "command")
            .unwrap_or_else(|| COMMAND_EXECUTION_TOOL_NAME.to_owned()),
        input_json: r.params.clone(),
        tool_use_id: string_field(&r.params, "itemId"),
    }
}

/// The neutral permission request a file-change approval projects to. Its params
/// carry no command or file path (only `itemId` / `grantRoot` / `reason`), so a
/// stable kind label names the interaction and the full params ride `input_json`.
fn file_change_approval(r: &ServerRequest) -> AgentPermissionRequest {
    AgentPermissionRequest {
        request_id: request_id_of(&r.id),
        tool_name: FILE_CHANGE_TOOL_NAME.to_owned(),
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

/// The `item` object a notification carries under `params.item`, if any.
fn item_of(params: &Value) -> Option<&Value> {
    params.get("item")
}

/// Project an `item/started` or `item/completed` into its neutral event(s) by an
/// explicit match on the real `item.type` (see the vendored `ThreadItem` oneOf):
///
/// - `agentMessage` → [`AgentEvent::AssistantDelta`] while streaming (a
///   non-empty `started` fragment) and [`AgentEvent::AssistantMessage`] once
///   completed;
/// - `commandExecution` / `fileChange` → [`AgentEvent::ToolStarted`] then
///   [`AgentEvent::ToolCompleted`], the full item (its real `command` / `cwd` /
///   `status` / `aggregatedOutput` / `exitCode` fields) riding the JSON payload;
/// - `userMessage` → nothing (the visible prompt is already surfaced as
///   [`AgentEvent::UserPromptAccepted`] at send time; re-emitting the echoed item
///   would duplicate it);
/// - `reasoning` → [`AgentEvent::ThinkingDelta`] while streaming and
///   [`AgentEvent::ThinkingMessage`] once completed — never an assistant
///   message, so reasoning is not mis-filed as reply text (see the module docs
///   for which of `content` / `summary` supplies the text);
/// - any other type → nothing (a safe skip, never a mis-filed tool call).
fn item_event(item: Option<&Value>, started: bool, at_ms: Option<i64>) -> Vec<AgentEvent> {
    let Some(item) = item else {
        return Vec::new();
    };
    let provider_item_id = string_field(item, "id").unwrap_or_default();
    match string_field(item, "type").unwrap_or_default().as_str() {
        AGENT_MESSAGE_ITEM_TYPE => agent_message_event(item, provider_item_id, started, at_ms),
        COMMAND_EXECUTION_ITEM_TYPE => tool_event(
            item,
            provider_item_id,
            COMMAND_EXECUTION_TOOL_NAME,
            started,
            at_ms,
        ),
        FILE_CHANGE_ITEM_TYPE => tool_event(
            item,
            provider_item_id,
            FILE_CHANGE_TOOL_NAME,
            started,
            at_ms,
        ),
        REASONING_ITEM_TYPE => reasoning_event(item, provider_item_id, started, at_ms),
        USER_MESSAGE_ITEM_TYPE => Vec::new(),
        _ => Vec::new(),
    }
}

/// Project an `agentMessage` item: a streaming [`AgentEvent::AssistantDelta`]
/// while it is still open (a non-empty `started` fragment) and the completed
/// [`AgentEvent::AssistantMessage`] once done. A started item with no text yet is
/// just "the assistant is about to speak" — nothing to show — so it emits
/// nothing rather than an empty delta.
fn agent_message_event(
    item: &Value,
    provider_item_id: String,
    started: bool,
    at_ms: Option<i64>,
) -> Vec<AgentEvent> {
    let text = string_field(item, "text").unwrap_or_default();
    if started {
        // A streaming fragment mints no persisted message (the completed item
        // does), so it carries no `at_ms`.
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
            at_ms,
        }]
    }
}

/// Project a `reasoning` item: a streaming [`AgentEvent::ThinkingDelta`] while
/// it is still open and the completed [`AgentEvent::ThinkingMessage`] once done —
/// the same started/completed split an `agentMessage` gets, on the
/// thinking-bearing pair so the model's reasoning is never mis-filed as its
/// reply.
///
/// An item with no reasoning text emits nothing. That covers both the `started`
/// frame (which announces the item before any reasoning has arrived) and a
/// completed item whose `content` and `summary` are both empty — the model
/// reasoned without exposing any of it, and an empty thinking block is noise
/// rather than a fact worth persisting.
fn reasoning_event(
    item: &Value,
    provider_item_id: String,
    started: bool,
    at_ms: Option<i64>,
) -> Vec<AgentEvent> {
    let text = reasoning_text(item);
    if text.is_empty() {
        return Vec::new();
    }
    if started {
        // A streaming fragment mints no persisted message (the completed item
        // does), so it carries no `at_ms`.
        vec![AgentEvent::ThinkingDelta {
            provider_item_id,
            text,
        }]
    } else {
        vec![AgentEvent::ThinkingMessage {
            provider_item_id,
            text,
            at_ms,
        }]
    }
}

/// The thinking text a `reasoning` item exposes: its raw `content` parts when
/// the server provides them, else its summarised `summary` parts. See the module
/// docs for why the raw text wins and why the fallback is the common case.
fn reasoning_text(item: &Value) -> String {
    let content = reasoning_parts(item, REASONING_CONTENT_FIELD);
    if !content.is_empty() {
        return content.join(REASONING_PART_SEPARATOR);
    }
    reasoning_parts(item, REASONING_SUMMARY_FIELD).join(REASONING_PART_SEPARATOR)
}

/// The non-empty string parts of a `reasoning` item's `content` / `summary`
/// array. Both are arrays of strings in the vendored `ReasoningThreadItem`;
/// anything else in the array is skipped rather than rendered, and blank parts
/// are dropped so joining them cannot leave a stray separator.
fn reasoning_parts(item: &Value, key: &str) -> Vec<String> {
    let Some(parts) = item.get(key).and_then(Value::as_array) else {
        return Vec::new();
    };
    parts
        .iter()
        .filter_map(Value::as_str)
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Project a tool item (`commandExecution` / `fileChange`) into its start/finish
/// events. `name` is the tool's stable *kind* label (the item type has no
/// separate tool-name field); the full item — carrying every real field, so
/// nothing is lost — rides the input (on start) / output (on finish) JSON.
fn tool_event(
    item: &Value,
    provider_item_id: String,
    name: &str,
    started: bool,
    at_ms: Option<i64>,
) -> Vec<AgentEvent> {
    if started {
        vec![AgentEvent::ToolStarted {
            provider_item_id,
            name: name.to_owned(),
            input_json: item.clone(),
            at_ms,
        }]
    } else {
        vec![AgentEvent::ToolCompleted {
            provider_item_id,
            output_json: item.clone(),
            at_ms,
        }]
    }
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

/// Read an integer field from a JSON object, returning `None` when absent or not
/// an integer. Used for the item lifecycle timestamps (`startedAtMs` /
/// `completedAtMs`, epoch milliseconds).
fn int_field(value: &Value, key: &str) -> Option<i64> {
    value.get(key).and_then(Value::as_i64)
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
        // The real `AgentMessageThreadItem` shape: `type: "agentMessage"`, the
        // reply under `text`, keyed by `id`.
        let events = translate_notification(&notification(
            "item/completed",
            json!({ "item": { "id": "item_1", "type": "agentMessage", "text": "hi", "phase": "final_answer" } }),
        ));
        assert_eq!(
            events,
            vec![AgentEvent::AssistantMessage {
                provider_item_id: "item_1".to_owned(),
                text: "hi".to_owned(),
                // This notification carries no `completedAtMs`, so `at_ms`
                // degrades to `None`.
                at_ms: None,
            }]
        );
    }

    #[test]
    fn agent_message_started_with_text_is_a_delta_and_empty_is_nothing() {
        let with_text = translate_notification(&notification(
            "item/started",
            json!({ "item": { "id": "i1", "type": "agentMessage", "text": "partial" } }),
        ));
        assert_eq!(
            with_text,
            vec![AgentEvent::AssistantDelta {
                provider_item_id: "i1".to_owned(),
                text: "partial".to_owned(),
            }]
        );

        // A started `agentMessage` announcing the item before any text (the real
        // server streams the body via `item/agentMessage/delta`) emits nothing.
        let empty = translate_notification(&notification(
            "item/started",
            json!({ "item": { "id": "i1", "type": "agentMessage" } }),
        ));
        assert!(empty.is_empty(), "an empty started message emits nothing");
    }

    #[test]
    fn an_agent_message_delta_notification_is_an_assistant_delta() {
        // The real `AgentMessageDeltaNotification`: the fragment under `delta`,
        // the item it extends under `itemId`.
        let events = translate_notification(&notification(
            "item/agentMessage/delta",
            json!({ "threadId": "thr_1", "turnId": "turn_1", "itemId": "i1", "delta": "chunk" }),
        ));
        assert_eq!(
            events,
            vec![AgentEvent::AssistantDelta {
                provider_item_id: "i1".to_owned(),
                text: "chunk".to_owned(),
            }]
        );
        // An empty delta emits nothing.
        assert!(translate_notification(&notification(
            "item/agentMessage/delta",
            json!({ "itemId": "i1", "delta": "" })
        ))
        .is_empty());
    }

    #[test]
    fn a_command_execution_item_is_a_tool_call_carrying_its_real_fields() {
        // The real `CommandExecutionThreadItem` shape.
        let started = translate_notification(&notification(
            "item/started",
            json!({ "item": {
                "id": "t1", "type": "commandExecution",
                "command": "ls", "cwd": "/tmp", "status": "inProgress", "commandActions": []
            } }),
        ));
        assert_eq!(
            started,
            vec![AgentEvent::ToolStarted {
                provider_item_id: "t1".to_owned(),
                name: "command_execution".to_owned(),
                input_json: json!({
                    "id": "t1", "type": "commandExecution",
                    "command": "ls", "cwd": "/tmp", "status": "inProgress", "commandActions": []
                }),
                at_ms: None,
            }],
            "the whole item rides input_json so every real field is preserved"
        );

        let completed = translate_notification(&notification(
            "item/completed",
            json!({ "item": {
                "id": "t1", "type": "commandExecution",
                "command": "ls", "cwd": "/tmp", "status": "completed",
                "commandActions": [], "aggregatedOutput": "a\nb", "exitCode": 0, "durationMs": 5
            } }),
        ));
        match &completed[..] {
            [AgentEvent::ToolCompleted {
                provider_item_id,
                output_json,
                at_ms: _,
            }] => {
                assert_eq!(provider_item_id, "t1");
                assert_eq!(output_json["exitCode"], 0);
                assert_eq!(output_json["aggregatedOutput"], "a\nb");
                assert_eq!(output_json["status"], "completed");
            }
            other => panic!("expected one ToolCompleted, got {other:?}"),
        }
    }

    #[test]
    fn a_file_change_item_is_a_tool_call() {
        // The real `FileChangeThreadItem` shape.
        let started = translate_notification(&notification(
            "item/started",
            json!({ "item": {
                "id": "fc1", "type": "fileChange", "status": "inProgress",
                "changes": [{ "path": "/x", "kind": "add" }]
            } }),
        ));
        assert_eq!(
            started,
            vec![AgentEvent::ToolStarted {
                provider_item_id: "fc1".to_owned(),
                name: "file_change".to_owned(),
                input_json: json!({
                    "id": "fc1", "type": "fileChange", "status": "inProgress",
                    "changes": [{ "path": "/x", "kind": "add" }]
                }),
                at_ms: None,
            }]
        );
    }

    #[test]
    fn item_lifecycle_timestamps_populate_the_events_at_ms() {
        // `item/started` carries `startedAtMs` and `item/completed` carries
        // `completedAtMs` as siblings of `item`; each is threaded onto the
        // projected event's neutral `at_ms`.
        let started = translate_notification(&notification(
            "item/started",
            json!({
                "threadId": "thr_1", "turnId": "turn_1", "startedAtMs": 1_700_000_000_123_i64,
                "item": { "id": "t1", "type": "commandExecution", "command": "ls", "status": "inProgress" }
            }),
        ));
        assert_eq!(
            started,
            vec![AgentEvent::ToolStarted {
                provider_item_id: "t1".to_owned(),
                name: "command_execution".to_owned(),
                input_json: json!({ "id": "t1", "type": "commandExecution", "command": "ls", "status": "inProgress" }),
                at_ms: Some(1_700_000_000_123),
            }]
        );

        let completed = translate_notification(&notification(
            "item/completed",
            json!({
                "threadId": "thr_1", "turnId": "turn_1", "completedAtMs": 1_700_000_005_456_i64,
                "item": { "id": "m1", "type": "agentMessage", "text": "done" }
            }),
        ));
        assert_eq!(
            completed,
            vec![AgentEvent::AssistantMessage {
                provider_item_id: "m1".to_owned(),
                text: "done".to_owned(),
                at_ms: Some(1_700_000_005_456),
            }]
        );
    }

    #[test]
    fn a_user_message_item_is_dropped_to_avoid_double_emitting_the_prompt() {
        // The prompt is already surfaced as `UserPromptAccepted` at send time, so
        // the echoed `UserMessageThreadItem` must not re-emit it.
        assert!(translate_notification(&notification(
            "item/started",
            json!({ "item": { "id": "u1", "type": "userMessage", "content": [{ "type": "text", "text": "hi" }] } })
        ))
        .is_empty());
        assert!(translate_notification(&notification(
            "item/completed",
            json!({ "item": { "id": "u1", "type": "userMessage", "content": [{ "type": "text", "text": "hi" }] } })
        ))
        .is_empty());
    }

    #[test]
    fn a_reasoning_item_and_its_deltas_become_thinking_not_misfiled() {
        // A reasoning item announced before any reasoning arrived has nothing to
        // show yet, so it emits nothing rather than an empty thinking block.
        assert!(translate_notification(&notification(
            "item/started",
            json!({ "item": { "id": "r1", "type": "reasoning", "summary": [], "content": [] } })
        ))
        .is_empty());

        // The completed item becomes a thinking-bearing event — never an
        // assistant message, which would misrepresent the model's internal
        // reasoning as its reply text.
        let completed = translate_notification(&notification(
            "item/completed",
            json!({
                "completedAtMs": 1_700_000_000_123_i64,
                "item": { "id": "r1", "type": "reasoning", "summary": ["s"], "content": ["c"] }
            }),
        ));
        assert_eq!(
            completed,
            vec![AgentEvent::ThinkingMessage {
                provider_item_id: "r1".to_owned(),
                text: "c".to_owned(),
                at_ms: Some(1_700_000_000_123),
            }]
        );

        // Both text-bearing reasoning deltas are streaming thinking fragments.
        for method in [
            "item/reasoning/textDelta",
            "item/reasoning/summaryTextDelta",
        ] {
            assert_eq!(
                translate_notification(&notification(
                    method,
                    json!({ "itemId": "r1", "delta": "thinking", "contentIndex": 0 })
                )),
                vec![AgentEvent::ThinkingDelta {
                    provider_item_id: "r1".to_owned(),
                    text: "thinking".to_owned(),
                }],
                "{method} must become a thinking fragment"
            );
        }
        // The part-added boundary carries no text of its own, so it emits
        // nothing; an empty fragment does not either.
        assert!(translate_notification(&notification(
            "item/reasoning/summaryPartAdded",
            json!({ "itemId": "r1", "summaryIndex": 1 })
        ))
        .is_empty());
        assert!(translate_notification(&notification(
            "item/reasoning/textDelta",
            json!({ "itemId": "r1", "delta": "", "contentIndex": 0 })
        ))
        .is_empty());

        // Nothing on the reasoning path is ever an assistant message or a tool.
        assert!(
            !completed.iter().any(|e| matches!(
                e,
                AgentEvent::AssistantMessage { .. }
                    | AgentEvent::AssistantDelta { .. }
                    | AgentEvent::ToolStarted { .. }
                    | AgentEvent::ToolCompleted { .. }
            )),
            "reasoning must never be mis-filed: {completed:?}"
        );
    }

    #[test]
    fn reasoning_prefers_raw_content_and_falls_back_to_the_summary() {
        let thinking_of = |item: Value| match translate_notification(&notification(
            "item/completed",
            json!({ "item": item }),
        ))
        .as_slice()
        {
            [AgentEvent::ThinkingMessage { text, .. }] => Some(text.clone()),
            [] => None,
            other => panic!("expected one thinking message, got {other:?}"),
        };

        // Raw reasoning wins when present: the summary condenses the same
        // reasoning, so showing both would show it twice.
        assert_eq!(
            thinking_of(json!({
                "id": "r1", "type": "reasoning",
                "content": ["raw one", "raw two"], "summary": ["condensed"]
            })),
            Some("raw one\n\nraw two".to_owned()),
            "parts join as separate paragraphs"
        );
        // Summary-only is the usual case for a hosted reasoning model, which
        // withholds its raw chain-of-thought — the fallback is what keeps the
        // thinking block non-empty in practice.
        assert_eq!(
            thinking_of(json!({
                "id": "r1", "type": "reasoning", "content": [], "summary": ["a", "b"]
            })),
            Some("a\n\nb".to_owned())
        );
        // Absent fields (both default to `[]` in the schema) and blank parts
        // degrade to nothing rather than an empty thinking block.
        assert_eq!(
            thinking_of(json!({ "id": "r1", "type": "reasoning" })),
            None
        );
        assert_eq!(
            thinking_of(json!({
                "id": "r1", "type": "reasoning", "content": ["", ""], "summary": [""]
            })),
            None
        );
        // A non-string part is skipped, never rendered as JSON.
        assert_eq!(
            thinking_of(json!({
                "id": "r1", "type": "reasoning", "summary": [{ "text": "x" }, "kept"]
            })),
            Some("kept".to_owned())
        );
    }

    #[test]
    fn an_unknown_item_type_is_skipped_not_treated_as_a_tool() {
        // A type this build does not model (e.g. `mcpToolCall`, `plan`) is a safe
        // skip — never mis-filed as a tool call, never a panic.
        for item_type in ["mcpToolCall", "plan", "webSearch", "somethingBrandNew"] {
            let started = translate_notification(&notification(
                "item/started",
                json!({ "item": { "id": "x1", "type": item_type, "status": "inProgress" } }),
            ));
            assert!(
                started.is_empty(),
                "an unknown item type `{item_type}` must not become a tool: {started:?}"
            );
        }
    }

    #[test]
    fn unmodeled_item_deltas_are_dropped() {
        for method in [
            "item/plan/delta",
            "item/commandExecution/outputDelta",
            "item/mcpToolCall/progress",
        ] {
            assert!(
                translate_notification(&notification(
                    method,
                    json!({ "itemId": "x", "delta": "y" })
                ))
                .is_empty(),
                "{method} must be dropped"
            );
        }
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
    fn a_command_execution_approval_becomes_a_permission_request() {
        // The real `item/commandExecution/requestApproval` params, as captured
        // from a live server turn: the command names the tool, `itemId` is the
        // tool-use id, and the full params (cwd, commandActions) ride input_json.
        let request = ServerRequest {
            id: json!("srv-1"),
            method: "item/commandExecution/requestApproval".to_owned(),
            params: json!({
                "threadId": "thr_1",
                "turnId": "turn_1",
                "itemId": "exec-9",
                "startedAtMs": 1_784_272_338_055_i64,
                "command": "/bin/zsh -lc date",
                "cwd": "/tmp",
                "commandActions": [{ "type": "unknown", "command": "date" }]
            }),
        };
        match classify_server_request(&request) {
            ServerRequestKind::Approval(req) => {
                assert_eq!(req.request_id, "srv-1");
                assert_eq!(
                    req.tool_name, "/bin/zsh -lc date",
                    "the command names the tool"
                );
                assert_eq!(req.tool_use_id, Some("exec-9".to_owned()));
                assert_eq!(
                    req.input_json["cwd"], "/tmp",
                    "the full params ride input_json"
                );
            }
            other => panic!("expected an approval, got {other:?}"),
        }
    }

    #[test]
    fn a_command_execution_approval_without_a_command_falls_back_to_a_kind_label() {
        let request = ServerRequest {
            id: json!(42),
            method: "item/commandExecution/requestApproval".to_owned(),
            params: json!({
                "threadId": "t", "turnId": "tn", "itemId": "exec-1", "startedAtMs": 0
            }),
        };
        match classify_server_request(&request) {
            ServerRequestKind::Approval(req) => {
                // A non-string id renders canonically so it still keys a lookup.
                assert_eq!(req.request_id, "42");
                assert_eq!(req.tool_name, "command_execution");
                assert_eq!(req.tool_use_id, Some("exec-1".to_owned()));
            }
            other => panic!("expected an approval, got {other:?}"),
        }
    }

    #[test]
    fn a_file_change_approval_becomes_a_permission_request() {
        let request = ServerRequest {
            id: json!("srv-2"),
            method: "item/fileChange/requestApproval".to_owned(),
            params: json!({
                "threadId": "thr_1",
                "turnId": "turn_1",
                "itemId": "fc-3",
                "startedAtMs": 0,
                "grantRoot": "/repo",
                "reason": "extra write access"
            }),
        };
        match classify_server_request(&request) {
            ServerRequestKind::Approval(req) => {
                assert_eq!(req.request_id, "srv-2");
                assert_eq!(
                    req.tool_name, "file_change",
                    "a file change has no command, so a kind label names it"
                );
                assert_eq!(req.tool_use_id, Some("fc-3".to_owned()));
                assert_eq!(req.input_json["grantRoot"], "/repo");
            }
            other => panic!("expected an approval, got {other:?}"),
        }
    }

    #[test]
    fn a_permissions_approval_is_unsupported_not_a_decision() {
        // The permissions approval's response is a GrantedPermissionProfile, not
        // a binary decision Delta can produce, so v1 surfaces it as unsupported
        // (and the adapter answers it) rather than fabricating a grant.
        let request = ServerRequest {
            id: json!("srv-3"),
            method: "item/permissions/requestApproval".to_owned(),
            params: json!({ "threadId": "thr_1", "itemId": "p1", "permissions": {} }),
        };
        match classify_server_request(&request) {
            ServerRequestKind::Unsupported { method, .. } => {
                assert_eq!(method, "item/permissions/requestApproval");
            }
            other => panic!("expected unsupported, got {other:?}"),
        }
    }

    #[test]
    fn an_unmodeled_server_request_is_unsupported() {
        // A method Delta does not model surfaces as unsupported, carrying its raw
        // params as detail so the adapter can log/annotate it.
        let request = ServerRequest {
            id: json!("srv-4"),
            method: "item/tool/requestUserInput".to_owned(),
            params: json!({ "threadId": "thr_1", "questions": [] }),
        };
        match classify_server_request(&request) {
            ServerRequestKind::Unsupported {
                method,
                detail_json,
            } => {
                assert_eq!(method, "item/tool/requestUserInput");
                assert_eq!(detail_json["threadId"], "thr_1");
            }
            other => panic!("expected unsupported, got {other:?}"),
        }
    }
}
