//! Projecting `item/started` and `item/completed` into neutral events, by the
//! item's `type`.

use serde_json::Value;

use delta_usecase::AgentEvent;

use super::{
    string_field, AGENT_MESSAGE_ITEM_TYPE, COMMAND_EXECUTION_ITEM_TYPE,
    COMMAND_EXECUTION_TOOL_NAME, FILE_CHANGE_ITEM_TYPE, FILE_CHANGE_TOOL_NAME, REASONING_ITEM_TYPE,
    USER_MESSAGE_ITEM_TYPE,
};

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
pub(super) const REASONING_PART_SEPARATOR: &str = "\n\n";

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
///   message, so reasoning is not mis-filed as reply text (see the [module docs]
///   for which of `content` / `summary` supplies the text);
/// - any other type → nothing (a safe skip, never a mis-filed tool call).
///
/// [module docs]: super
pub(super) fn item_event(
    item: Option<&Value>,
    started: bool,
    at_ms: Option<i64>,
) -> Vec<AgentEvent> {
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
/// the server provides them, else its summarised `summary` parts. See the [module
/// docs] for why the raw text wins and why the fallback is the common case.
///
/// [module docs]: super
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
