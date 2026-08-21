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

use serde_json::Value;

use delta_usecase::{
    AgentEvent, AgentFileChange, AgentFileChangeDetail, AgentFileChangeKind,
    AgentPermissionRequest, AgentTokenUsage, RateLimitWindow, TurnStatus,
};

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
const METHOD_REASONING_SUMMARY_PART_ADDED: &str = "item/reasoning/summaryPartAdded";

/// The notification (`FileChangePatchUpdatedNotification`) that **revises** a
/// file-change item's proposed patch after `item/started` announced it:
/// `{ itemId, threadId, turnId, changes }`. It carries the whole replacement
/// change list, so a tracker that only read `item/started` would show a stale
/// diff on the approval raised afterwards.
const METHOD_FILE_CHANGE_PATCH_UPDATED: &str = "item/fileChange/patchUpdated";

/// The `FileUpdateChange` field naming the path that changes.
const CHANGE_PATH_FIELD: &str = "path";
/// The `FileUpdateChange` field carrying the unified diff of the change.
const CHANGE_DIFF_FIELD: &str = "diff";
/// The `FileUpdateChange` field carrying the change's kind — a
/// `PatchChangeKind`, i.e. an *object* whose `type` is the discriminant, not a
/// bare string.
const CHANGE_KIND_FIELD: &str = "kind";
/// The `PatchChangeKind` discriminant field (`add` / `delete` / `update`).
const CHANGE_KIND_TYPE_FIELD: &str = "type";
/// The array of `FileUpdateChange`s, carried both by a `FileChangeThreadItem`
/// and by [`METHOD_FILE_CHANGE_PATCH_UPDATED`] under the same key.
const CHANGES_FIELD: &str = "changes";

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

/// The account-scoped rate-limit state observed on one `codex app-server`
/// connection, and the merge the vendored schema requires of its clients.
///
/// `account/rateLimits/updated` is a **sparse rolling update**: the schema says
/// in as many words that a client must merge the values it carries into the
/// snapshot it last observed, and that a null field "does not clear a previously
/// observed value" (see [`merge_window`] for what that means field by field).
/// This type is where that merge lives — at the provider's own edge, since the
/// merge rule is the provider's, not the core's.
///
/// It is connection-scoped rather than session-scoped because the account is:
/// one shared app-server connection hosts every Codex session, and these limits
/// describe the account behind it.
#[derive(Debug, Default)]
pub struct AccountRateLimits {
    /// The most significant window the account reports, as last observed.
    primary: Option<RateLimitWindow>,
    /// The secondary window, as last observed.
    secondary: Option<RateLimitWindow>,
}

impl AccountRateLimits {
    /// Fold one account-scoped notification into the observed state and project
    /// it into neutral events.
    ///
    /// Yields nothing for any notification this build does not model — including
    /// a rate-limit frame with no `rateLimits` object at all, which states
    /// nothing and must therefore not be turned into an empty window list that
    /// would wipe the display.
    pub fn translate(&mut self, n: &Notification) -> Vec<AgentEvent> {
        match n.method.as_str() {
            METHOD_ACCOUNT_RATE_LIMITS => match n.params.get("rateLimits") {
                Some(snapshot) => vec![AgentEvent::RateLimitsUpdated {
                    windows: self.merge(snapshot),
                }],
                None => Vec::new(),
            },
            _ => Vec::new(),
        }
    }

    /// Merge a sparse `RateLimitSnapshot` into the observed windows and return
    /// them, most significant first. A window the account does not report at all
    /// is simply absent from the list (its row disappears rather than showing
    /// zero).
    fn merge(&mut self, snapshot: &Value) -> Vec<RateLimitWindow> {
        self.primary = merge_window(self.primary.take(), snapshot.get("primary"));
        self.secondary = merge_window(self.secondary.take(), snapshot.get("secondary"));
        [self.primary.clone(), self.secondary.clone()]
            .into_iter()
            .flatten()
            .collect()
    }
}

/// Merge one sparse window onto the last observed one.
///
/// An absent or null window leaves the previous observation untouched. A present
/// one overwrites only the fields it actually carries: `usedPercent` is required
/// by the schema, while `resetsAt` and `windowDurationMins` are nullable and a
/// null there means "unavailable in this rolling update", not "cleared".
fn merge_window(
    previous: Option<RateLimitWindow>,
    update: Option<&Value>,
) -> Option<RateLimitWindow> {
    let Some(update) = update.filter(|value| !value.is_null()) else {
        // Nothing said about this window: keep exactly what was last observed
        // (including having observed nothing).
        return previous;
    };
    let previous = previous.unwrap_or_default();
    Some(RateLimitWindow {
        // Minutes on the wire, seconds in the neutral model — the unit the
        // browser labels and paces the row with.
        duration_seconds: int_field(update, "windowDurationMins")
            .map(|minutes| minutes * 60)
            .or(previous.duration_seconds),
        used_percentage: update
            .get("usedPercent")
            .and_then(Value::as_f64)
            .or(previous.used_percentage),
        resets_at: int_field(update, "resetsAt").or(previous.resets_at),
    })
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
        // A command execution changes no files up front, so there is nothing to
        // correlate: the command itself already names what it would do.
        file_change: None,
        // Only a file-change approval asks for a write root.
        grant_root: None,
    }
}

/// The neutral permission request a file-change approval projects to. Its params
/// carry no command or file path (only `itemId` / `grantRoot` / `reason`), so a
/// stable kind label names the interaction and the full params ride `input_json`.
///
/// The paths, kinds and diffs the card actually wants are **not** here: they
/// arrived earlier, on the `item/started` for the same `itemId`. Correlating the
/// two needs per-session state, which a pure translator has none of, so this
/// leaves [`AgentPermissionRequest::file_change`] unset and the adapter completes
/// it through [`with_file_change_detail`] when it knows the item. An approval
/// whose item was never seen keeps the `None` — the deliberate fallback to the
/// input summary.
///
/// `grantRoot` is the exception that is read here and not correlated: it belongs
/// to the approval itself, not to the item, so it is lifted onto
/// [`AgentPermissionRequest::grant_root`] straight away and reaches the card
/// even when no item is ever found for the `itemId`. It is also the broadest
/// thing the request asks for — writes anywhere under that root for the rest of
/// the session, well past the files the item lists — so it is the last thing
/// that may go missing.
fn file_change_approval(r: &ServerRequest) -> AgentPermissionRequest {
    AgentPermissionRequest {
        request_id: request_id_of(&r.id),
        tool_name: FILE_CHANGE_TOOL_NAME.to_owned(),
        input_json: r.params.clone(),
        tool_use_id: string_field(&r.params, "itemId"),
        file_change: None,
        grant_root: string_field(&r.params, "grantRoot"),
    }
}

/// Complete a file-change approval with the change set correlated from its item,
/// so the browser card can name the affected files and show their diffs.
///
/// The `reason` comes from the approval's own params (which already ride
/// `input_json`) while the changes come from the item — the two halves of the
/// same question, joined here. Called only with changes an item actually stated;
/// a request left untouched keeps `file_change: None` and falls back to the input
/// summary.
pub fn with_file_change_detail(
    request: &mut AgentPermissionRequest,
    changes: Vec<AgentFileChange>,
) {
    let reason = string_field(&request.input_json, "reason");
    request.file_change = Some(AgentFileChangeDetail { changes, reason });
}

/// The file-change item a notification announces, as `(item id, changes)` — the
/// pair the adapter keys its correlation map by.
///
/// Two notifications state a file-change item's proposed patch, and both are
/// read here so the map is correct at the moment an approval arrives:
///
/// - `item/started` for a `fileChange` item (`params.item`, a
///   `FileChangeThreadItem`) — the first statement of the patch;
/// - [`METHOD_FILE_CHANGE_PATCH_UPDATED`] (`params.itemId` / `params.changes`) —
///   a **replacement** of it, so the caller overwrites rather than merges.
///
/// `None` for every other notification, for an item of another type, and for an
/// item with no id: nothing to correlate, so nothing is tracked.
pub fn file_change_item(n: &Notification) -> Option<(String, Vec<AgentFileChange>)> {
    let (id, changes) = match n.method.as_str() {
        "item/started" => {
            let item = item_of(&n.params)?;
            if string_field(item, "type").as_deref() != Some(FILE_CHANGE_ITEM_TYPE) {
                return None;
            }
            (string_field(item, "id")?, item.get(CHANGES_FIELD))
        }
        METHOD_FILE_CHANGE_PATCH_UPDATED => (
            string_field(&n.params, "itemId")?,
            n.params.get(CHANGES_FIELD),
        ),
        _ => return None,
    };
    Some((id, file_changes(changes)))
}

/// The id of the item an `item/completed` announces, so the adapter can drop the
/// correlation entry the item no longer needs. `None` for any other notification
/// (or a completed item with no id).
pub fn completed_item_id(n: &Notification) -> Option<String> {
    match n.method.as_str() {
        "item/completed" => string_field(item_of(&n.params)?, "id"),
        _ => None,
    }
}

/// Project a `FileUpdateChange` array into the neutral change list.
///
/// An entry missing its required `path` is skipped: a change Delta cannot name
/// is worse than no row at all, since the card's whole job is to say which file
/// is affected. A missing `diff` degrades to the empty string (the expander then
/// has nothing to show) rather than dropping the path.
fn file_changes(changes: Option<&Value>) -> Vec<AgentFileChange> {
    let Some(changes) = changes.and_then(Value::as_array) else {
        return Vec::new();
    };
    changes
        .iter()
        .filter_map(|change| {
            Some(AgentFileChange {
                path: string_field(change, CHANGE_PATH_FIELD)?,
                kind: file_change_kind(change.get(CHANGE_KIND_FIELD)),
                diff: string_field(change, CHANGE_DIFF_FIELD).unwrap_or_default(),
            })
        })
        .collect()
}

/// Map a `PatchChangeKind` onto the neutral kind. The wire shape is an object
/// tagged by `type` (`{"type":"update","move_path":null}`), not a bare string.
/// A kind this build does not model — including an absent or malformed one —
/// yields `None`, so the path and diff still show without a fabricated label.
fn file_change_kind(kind: Option<&Value>) -> Option<AgentFileChangeKind> {
    match string_field(kind?, CHANGE_KIND_TYPE_FIELD)?.as_str() {
        "add" => Some(AgentFileChangeKind::Add),
        "update" => Some(AgentFileChangeKind::Update),
        "delete" => Some(AgentFileChangeKind::Delete),
        _ => None,
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

    /// The token-usage breakdown the real server sends: every field of
    /// `TokenUsageBreakdown` is required, so a fixture that omits one would not
    /// be a shape the server can produce.
    fn breakdown(total: u64, input: u64) -> Value {
        json!({
            "totalTokens": total,
            "inputTokens": input,
            "cachedInputTokens": 0,
            "outputTokens": 0,
            "reasoningOutputTokens": 0,
        })
    }

    #[test]
    fn thread_token_usage_becomes_a_usage_event_with_a_percentage_of_the_context_window() {
        // The real `ThreadTokenUsageUpdatedNotification`: thread-scoped, so it
        // arrives through the per-thread demux and must NOT fall into the
        // catch-all that used to swallow it.
        let events = translate_notification(&notification(
            "thread/tokenUsage/updated",
            json!({
                "threadId": "thr_1",
                "turnId": "turn_1",
                "tokenUsage": {
                    // The proportions a real session reaches: the running
                    // `total` is already 2.5x the window while the last call —
                    // the conversation actually in the window — is a quarter of
                    // it.
                    "total": breakdown(500_000, 480_000),
                    "last": breakdown(50_000, 48_000),
                    "modelContextWindow": 200_000,
                }
            }),
        ));
        assert_eq!(
            events,
            vec![AgentEvent::TokenUsageUpdated {
                usage: AgentTokenUsage {
                    // 50_000 / 200_000 — computed here, at Codex's own edge,
                    // because the server never sends a percentage.
                    context_used_percentage: Some(25.0),
                    context_window_size: Some(200_000),
                    // `last`, never the running `total`: the latter would read
                    // 250% here, and only ever climbs.
                    context_current_usage: Some(50_000),
                    // The one genuinely cumulative number, so this one is
                    // `total`'s.
                    total_input_tokens: Some(480_000),
                },
            }]
        );
    }

    #[test]
    fn token_usage_without_a_context_window_reports_no_percentage() {
        // `modelContextWindow` is nullable. With nothing to divide by there is
        // no honest percentage, so the counts are still reported and the
        // percentage is omitted — never NaN, never a fabricated 0%.
        for window in [json!(null), json!(0)] {
            let events = translate_notification(&notification(
                "thread/tokenUsage/updated",
                json!({
                    "threadId": "thr_1",
                    "turnId": "turn_1",
                    "tokenUsage": {
                        "total": breakdown(500_000, 480_000),
                        "last": breakdown(50_000, 48_000),
                        "modelContextWindow": window,
                    }
                }),
            ));
            assert_eq!(
                events,
                vec![AgentEvent::TokenUsageUpdated {
                    usage: AgentTokenUsage {
                        context_used_percentage: None,
                        context_window_size: None,
                        context_current_usage: Some(50_000),
                        total_input_tokens: Some(480_000),
                    },
                }],
                "modelContextWindow {window} must yield no percentage"
            );
        }

        // A frame with no `tokenUsage` at all states nothing.
        assert!(translate_notification(&notification(
            "thread/tokenUsage/updated",
            json!({ "threadId": "thr_1", "turnId": "turn_1" })
        ))
        .is_empty());
    }

    #[test]
    fn account_rate_limits_become_duration_identified_windows() {
        // The real `AccountRateLimitsUpdatedNotification`: note the absence of
        // any `threadId` — this frame belongs to the account, not a thread.
        let mut limits = AccountRateLimits::default();
        let events = limits.translate(&notification(
            "account/rateLimits/updated",
            json!({
                "rateLimits": {
                    "primary": { "usedPercent": 21, "resetsAt": 1_700_000_000_i64, "windowDurationMins": 300 },
                    "secondary": { "usedPercent": 4, "resetsAt": 1_700_500_000_i64, "windowDurationMins": 10_080 },
                    "planType": "pro",
                }
            }),
        ));
        assert_eq!(
            events,
            vec![AgentEvent::RateLimitsUpdated {
                windows: vec![
                    RateLimitWindow {
                        // 300 minutes on the wire is 5 hours of neutral window.
                        duration_seconds: Some(5 * 60 * 60),
                        used_percentage: Some(21.0),
                        resets_at: Some(1_700_000_000),
                    },
                    RateLimitWindow {
                        duration_seconds: Some(7 * 24 * 60 * 60),
                        used_percentage: Some(4.0),
                        resets_at: Some(1_700_500_000),
                    },
                ],
            }]
        );
    }

    #[test]
    fn a_sparse_rate_limit_update_merges_and_never_clears_what_was_observed() {
        // The vendored schema is explicit: a rolling update is sparse, and a
        // null field "does not clear a previously observed value".
        let mut limits = AccountRateLimits::default();
        limits.translate(&notification(
            "account/rateLimits/updated",
            json!({
                "rateLimits": {
                    "primary": { "usedPercent": 21, "resetsAt": 1_700_000_000_i64, "windowDurationMins": 300 },
                    "secondary": { "usedPercent": 4, "resetsAt": 1_700_500_000_i64, "windowDurationMins": 10_080 },
                }
            }),
        ));

        // A second update naming only `primary` — and, within it, only the
        // required `usedPercent`. Everything it does not carry survives: the
        // whole `secondary` window, and `primary`'s own reset time and duration.
        let events = limits.translate(&notification(
            "account/rateLimits/updated",
            json!({
                "rateLimits": {
                    "primary": { "usedPercent": 37, "resetsAt": null, "windowDurationMins": null },
                    "secondary": null,
                }
            }),
        ));
        assert_eq!(
            events,
            vec![AgentEvent::RateLimitsUpdated {
                windows: vec![
                    RateLimitWindow {
                        duration_seconds: Some(5 * 60 * 60),
                        used_percentage: Some(37.0),
                        resets_at: Some(1_700_000_000),
                    },
                    RateLimitWindow {
                        duration_seconds: Some(7 * 24 * 60 * 60),
                        used_percentage: Some(4.0),
                        resets_at: Some(1_700_500_000),
                    },
                ],
            }]
        );

        // An update that omits both keys entirely leaves everything standing.
        let events = limits.translate(&notification(
            "account/rateLimits/updated",
            json!({ "rateLimits": { "planType": "pro" } }),
        ));
        let AgentEvent::RateLimitsUpdated { windows } = &events[0] else {
            panic!("expected a rate-limit event, got {events:?}");
        };
        assert_eq!(windows.len(), 2, "an omitted window is not a cleared one");
        assert_eq!(windows[0].used_percentage, Some(37.0));
    }

    #[test]
    fn an_account_frame_that_states_nothing_yields_no_event() {
        let mut limits = AccountRateLimits::default();
        // No `rateLimits` object: nothing was observed, so nothing is emitted —
        // an empty window list here would wipe the display on a malformed frame.
        assert!(limits
            .translate(&notification("account/rateLimits/updated", json!({})))
            .is_empty());
        // A method this build does not model is dropped, not guessed at.
        assert!(limits
            .translate(&notification(
                "account/somethingNew",
                json!({ "whatever": true })
            ))
            .is_empty());
    }

    #[test]
    fn the_account_frame_is_not_thread_scoped() {
        // Guard against the fake (and any future scenario) drifting into
        // stamping a `threadId` onto the account frame: the thread-scoped
        // translator must not model it, because in production it never reaches
        // that path at all.
        assert!(translate_notification(&notification(
            "account/rateLimits/updated",
            json!({ "rateLimits": { "primary": { "usedPercent": 21 } } })
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
                assert_eq!(
                    req.grant_root.as_deref(),
                    Some("/repo"),
                    "the broadest ask is lifted out of the params, not left buried in them"
                );
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

    // --- File-change correlation ---------------------------------------------

    fn update_change(path: &str, diff: &str) -> Value {
        json!({ "path": path, "kind": { "type": "update" }, "diff": diff })
    }

    #[test]
    fn an_item_started_file_change_yields_its_id_and_changes() {
        let n = notification(
            "item/started",
            json!({
                "item": {
                    "id": "fc-1",
                    "type": "fileChange",
                    "status": "inProgress",
                    "changes": [
                        { "path": "a.rs", "kind": { "type": "add" }, "diff": "+a" },
                        { "path": "b.rs", "kind": { "type": "delete" }, "diff": "-b" },
                        update_change("c.rs", "~c"),
                    ],
                },
            }),
        );

        assert_eq!(
            file_change_item(&n),
            Some((
                "fc-1".to_owned(),
                vec![
                    AgentFileChange {
                        path: "a.rs".to_owned(),
                        kind: Some(AgentFileChangeKind::Add),
                        diff: "+a".to_owned(),
                    },
                    AgentFileChange {
                        path: "b.rs".to_owned(),
                        kind: Some(AgentFileChangeKind::Delete),
                        diff: "-b".to_owned(),
                    },
                    AgentFileChange {
                        path: "c.rs".to_owned(),
                        kind: Some(AgentFileChangeKind::Update),
                        diff: "~c".to_owned(),
                    },
                ],
            )),
        );
    }

    #[test]
    fn an_item_of_another_type_is_not_tracked() {
        let n = notification(
            "item/started",
            json!({ "item": { "id": "m1", "type": "agentMessage" } }),
        );

        assert_eq!(file_change_item(&n), None);
    }

    #[test]
    fn a_patch_update_yields_the_replacement_changes() {
        let n = notification(
            METHOD_FILE_CHANGE_PATCH_UPDATED,
            json!({
                "itemId": "fc-1",
                "threadId": "thr_1",
                "turnId": "turn_1",
                "changes": [update_change("a.rs", "revised")],
            }),
        );

        assert_eq!(
            file_change_item(&n),
            Some((
                "fc-1".to_owned(),
                vec![AgentFileChange {
                    path: "a.rs".to_owned(),
                    kind: Some(AgentFileChangeKind::Update),
                    diff: "revised".to_owned(),
                }],
            )),
        );
        assert!(
            translate_notification(&n).is_empty(),
            "the revision itself projects to no neutral event"
        );
    }

    #[test]
    fn an_unmodeled_kind_keeps_the_path_and_diff_without_a_label() {
        // A newer server naming a kind this build does not model must not cost
        // the user the path — that is the part the answer usually turns on.
        let n = notification(
            "item/started",
            json!({
                "item": {
                    "id": "fc-1",
                    "type": "fileChange",
                    "changes": [
                        { "path": "a.rs", "kind": { "type": "teleport" }, "diff": "?" },
                        { "kind": { "type": "add" }, "diff": "no path" },
                    ],
                },
            }),
        );

        assert_eq!(
            file_change_item(&n),
            Some((
                "fc-1".to_owned(),
                vec![AgentFileChange {
                    path: "a.rs".to_owned(),
                    kind: None,
                    diff: "?".to_owned(),
                }],
            )),
            "an unknown kind loses its label; a change with no path is dropped entirely"
        );
    }

    #[test]
    fn a_completed_item_reports_the_id_to_forget() {
        let completed = notification(
            "item/completed",
            json!({ "item": { "id": "fc-1", "type": "fileChange", "changes": [] } }),
        );
        let started = notification(
            "item/started",
            json!({ "item": { "id": "fc-1", "type": "fileChange", "changes": [] } }),
        );

        assert_eq!(completed_item_id(&completed), Some("fc-1".to_owned()));
        assert_eq!(
            completed_item_id(&started),
            None,
            "only a completion retires an entry"
        );
    }

    #[test]
    fn completing_a_file_change_approval_joins_its_changes_with_its_reason() {
        // The changes come from the item, the reason from the approval's own
        // params — the two halves of the same question.
        let request = ServerRequest {
            id: json!("srv-9"),
            method: METHOD_FILE_CHANGE_APPROVAL.to_owned(),
            params: json!({
                "threadId": "thr_1",
                "turnId": "turn_1",
                "itemId": "fc-1",
                "startedAtMs": 0,
                "reason": "extra write access",
            }),
        };
        let ServerRequestKind::Approval(mut approval) = classify_server_request(&request) else {
            panic!("a file-change approval is modeled");
        };
        assert_eq!(
            approval.file_change, None,
            "the pure classification cannot correlate; that is the adapter's job"
        );

        with_file_change_detail(
            &mut approval,
            vec![AgentFileChange {
                path: "a.rs".to_owned(),
                kind: Some(AgentFileChangeKind::Update),
                diff: "~a".to_owned(),
            }],
        );

        assert_eq!(
            approval.file_change,
            Some(AgentFileChangeDetail {
                changes: vec![AgentFileChange {
                    path: "a.rs".to_owned(),
                    kind: Some(AgentFileChangeKind::Update),
                    diff: "~a".to_owned(),
                }],
                reason: Some("extra write access".to_owned()),
            }),
        );
    }
}
