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
//!
//! **Usage.** Codex reports what it is spending on two independent
//! notifications, and the two are scoped differently:
//!
//! - `thread/tokenUsage/updated` is thread-scoped, so it flows through the
//!   normal per-thread demux and becomes an
//!   [`AgentEvent::TokenUsageUpdated`] ([`token_usage`]);
//! - `account/rateLimits/updated` carries **no `threadId`** at all — it
//!   describes the account behind the shared connection, not any one thread —
//!   so it never reaches [`translate_notification`]. It is folded by
//!   [`AccountRateLimits`], which also implements the sparse-merge rule the
//!   vendored schema demands of clients, and becomes an
//!   [`AgentEvent::RateLimitsUpdated`].
//!
//! Both are observability only: they complete no message and touch no turn.
//!
//! [`AgentPermissionRequest`]: delta_usecase::AgentPermissionRequest

use serde_json::Value;

use delta_usecase::{AgentEvent, AgentTokenUsage, TurnStatus};

use crate::wire::Notification;

mod account_rate_limits;
pub use account_rate_limits::AccountRateLimits;

mod server_request_kind;
pub use server_request_kind::ServerRequestKind;

mod approval;
pub use approval::classify_server_request;

mod file_change;
pub use file_change::{completed_item_id, file_change_item, with_file_change_detail};

mod item;
use item::item_event;

#[cfg(test)]
mod tests;

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

/// The streaming-delta method (`AgentMessageDeltaNotification`) that carries a
/// fragment of an assistant message, under `params.itemId` / `params.delta`.
const METHOD_AGENT_MESSAGE_DELTA: &str = "item/agentMessage/delta";

/// The thread-scoped usage notification (`ThreadTokenUsageUpdatedNotification`):
/// `{ threadId, turnId, tokenUsage }`, where `tokenUsage` is a
/// `ThreadTokenUsage`.
const METHOD_THREAD_TOKEN_USAGE: &str = "thread/tokenUsage/updated";
/// The account-scoped rate-limit notification
/// (`AccountRateLimitsUpdatedNotification`): `{ rateLimits }`, a sparse
/// `RateLimitSnapshot`. It carries **no** `threadId` — see
/// [`AccountRateLimits`].
const METHOD_ACCOUNT_RATE_LIMITS: &str = "account/rateLimits/updated";

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
///
/// [`REASONING_PART_SEPARATOR`]: item::REASONING_PART_SEPARATOR
const METHOD_REASONING_SUMMARY_PART_ADDED: &str = "item/reasoning/summaryPartAdded";

/// The notification (`FileChangePatchUpdatedNotification`) that **revises** a
/// file-change item's proposed patch after `item/started` announced it:
/// `{ itemId, threadId, turnId, changes }`. It carries the whole replacement
/// change list, so a tracker that only read `item/started` would show a stale
/// diff on the approval raised afterwards.
const METHOD_FILE_CHANGE_PATCH_UPDATED: &str = "item/fileChange/patchUpdated";

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
        METHOD_THREAD_TOKEN_USAGE => token_usage(&n.params),
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
        // The revised patch of a file-change item projects to no neutral event
        // of its own (the item's completion carries the final change set), but
        // it is not ignored either: the adapter reads it through
        // [`file_change_item`] so an approval raised afterwards shows the
        // revised diff rather than the one `item/started` announced.
        METHOD_FILE_CHANGE_PATCH_UPDATED => Vec::new(),
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

/// Project a `thread/tokenUsage/updated` into a neutral
/// [`AgentEvent::TokenUsageUpdated`].
///
/// The vendored `ThreadTokenUsage` carries two `TokenUsageBreakdown`s — `total`
/// (every model call this thread has made, added up) and `last` (the most
/// recent call's) — plus an optional `modelContextWindow`.
///
/// **Occupancy comes from `last`; `total` is a running sum and would be
/// nonsense here.** Every call re-sends the whole conversation, so `last`'s
/// input side *is* the conversation currently in the window and
/// `last.totalTokens` is that plus the reply it drew — which is exactly what
/// occupies the window. `total` instead accumulates every call's tokens, so it
/// passes the window size after a handful of turns and never comes back: three
/// real Codex rollouts against a 258,400-token window end at
/// `total.totalTokens` = 576k / 1.31M / 1.74M (223% / 507% / 674% of the
/// window) while their `last.totalTokens` sit at 48k / 50k / 73k (19–28%). A
/// bar driven by `total` would therefore read "full" on every session that ran
/// more than a few turns. `total` still answers the cumulative question —
/// how many input tokens this session has sent — which is the one
/// [`AgentTokenUsage::total_input_tokens`] asks, so that field alone reads it.
///
/// **The percentage is computed here, at Codex's own edge, or not at all.** The
/// server reports absolute counts and never a percentage, so the adapter
/// divides `last.totalTokens` by `modelContextWindow` — and when the server
/// reports no window (the field is nullable, and zero would be meaningless
/// besides) it reports *no percentage*, rather than a NaN, an infinity or a
/// fabricated 0%.
fn token_usage(params: &Value) -> Vec<AgentEvent> {
    let Some(usage) = params.get("tokenUsage") else {
        return Vec::new();
    };
    let in_context = usage
        .get("last")
        .and_then(|last| uint_field(last, "totalTokens"));
    let context_window_size = uint_field(usage, "modelContextWindow").filter(|size| *size > 0);
    vec![AgentEvent::TokenUsageUpdated {
        usage: AgentTokenUsage {
            context_used_percentage: in_context.zip(context_window_size).map(|(used, size)| {
                // Both are counts, so the ratio is exact enough in `f64` for a
                // display percentage; the browser rounds it for the readout.
                used as f64 / size as f64 * 100.0
            }),
            context_window_size,
            context_current_usage: in_context,
            total_input_tokens: usage
                .get("total")
                .and_then(|total| uint_field(total, "inputTokens")),
        },
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
/// `completedAtMs`, epoch milliseconds) and the rate-limit window's reset time /
/// duration.
fn int_field(value: &Value, key: &str) -> Option<i64> {
    value.get(key).and_then(Value::as_i64)
}

/// Read an unsigned integer field from a JSON object. Used for the token counts,
/// which are counts and can never be negative; a negative value on the wire is
/// nonsense and degrades to `None` rather than wrapping.
fn uint_field(value: &Value, key: &str) -> Option<u64> {
    value.get(key).and_then(Value::as_u64)
}
