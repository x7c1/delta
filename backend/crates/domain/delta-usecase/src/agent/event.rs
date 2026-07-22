//! [`AgentEvent`]: the single, provider-neutral fact stream the core reasons
//! over.
//!
//! This is the one external truth the session actor / Turn FSM will consume
//! (Phase B routes the runtime through it). Its shape is modelled on what a
//! structured app-server already emits; a lossy provider such as Claude Code
//! *reconstructs* this stream from its hooks and JSONL transcript inside its
//! adapter, so the core never sees provider-specific richness.
//!
//! Provider-specific identifiers are carried verbatim as opaque strings
//! (`provider_session_id`, `provider_turn_id`, `provider_item_id`,
//! `provider_message_id`): the core stores and correlates them but never parses
//! their internal structure.

use serde_json::Value;

use crate::interactor::PermissionDecision;

/// Why a session's underlying agent process ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionEndReason {
    /// The session was closed deliberately (the pane/process was torn down).
    Closed,
    /// The agent process exited on its own or was lost unexpectedly.
    ProcessExited,
    /// The session ended in a failure state (e.g. a spawn that never bound).
    Failed,
}

/// How a turn finished. Mirrors the app-server `turn/completed` status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnStatus {
    /// The turn ran to completion.
    Completed,
    /// The user interrupted the turn before it completed.
    Interrupted,
    /// The turn ended in an error.
    Failed,
}

/// A tool-permission request surfaced by the agent, in provider-neutral form.
///
/// This is distinct from [`delta_model::PermissionRequest`], which is the
/// *persisted* row (with a database id and timestamps). This type is the live
/// request as it crosses the adapter boundary: the correlation ids the adapter
/// knows, the tool name, and its input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentPermissionRequest {
    /// The adapter-scoped id used to resolve this request with a decision.
    pub request_id: String,
    /// The tool the agent is asking to run.
    pub tool_name: String,
    /// The tool input, as structured JSON.
    pub input_json: Value,
    /// The provider's id for the tool call this request gates, when the
    /// provider exposes one (Claude's `tool_use_id`; `None` when absent).
    pub tool_use_id: Option<String>,
}

/// The neutral event stream the core reasons over, regardless of provider.
///
/// Every variant is an *observed fact* about the agent session. The core turns
/// these into its own state transitions; the adapter is responsible for
/// producing them faithfully (a structured provider forwards them almost
/// directly, Claude Code reconstructs them from hooks + transcript).
#[derive(Debug, Clone, PartialEq)]
pub enum AgentEvent {
    /// The session is established; `provider_session_id` is the provider's own
    /// id for it (Claude: the pinned `--session-id`; Codex: the returned
    /// `thr_...`).
    SessionStarted { provider_session_id: String },
    /// The session's agent process has ended.
    SessionEnded { reason: SessionEndReason },
    /// A turn has begun. `provider_turn_id` is `None` for providers that do not
    /// name turns (Claude confirms a turn via the prompt echo without an id).
    TurnStarted { provider_turn_id: Option<String> },
    /// The user's prompt was accepted into the turn.
    UserPromptAccepted {
        provider_message_id: Option<String>,
        text: String,
        /// When this message fact occurred, in epoch milliseconds, when the
        /// provider exposes a per-message time (`None` otherwise). A neutral,
        /// optional message fact — carried verbatim, not parsed. Codex fills it
        /// from the item's `startedAtMs` / `completedAtMs`; Claude leaves it
        /// `None` (its `created_at` comes from the transcript fold).
        at_ms: Option<i64>,
    },
    /// A streaming fragment of an assistant message.
    AssistantDelta {
        provider_item_id: String,
        text: String,
    },
    /// A completed assistant message.
    AssistantMessage {
        provider_item_id: String,
        text: String,
        /// When this message fact occurred, in epoch milliseconds, when the
        /// provider exposes a per-message time (`None` otherwise). A neutral,
        /// optional message fact — carried verbatim, not parsed. Codex fills it
        /// from the item's `startedAtMs` / `completedAtMs`; Claude leaves it
        /// `None` (its `created_at` comes from the transcript fold).
        at_ms: Option<i64>,
    },
    /// A tool call started.
    ToolStarted {
        provider_item_id: String,
        name: String,
        input_json: Value,
        /// When this message fact occurred, in epoch milliseconds, when the
        /// provider exposes a per-message time (`None` otherwise). A neutral,
        /// optional message fact — carried verbatim, not parsed. Codex fills it
        /// from the item's `startedAtMs` / `completedAtMs`; Claude leaves it
        /// `None` (its `created_at` comes from the transcript fold).
        at_ms: Option<i64>,
    },
    /// A tool call completed.
    ToolCompleted {
        provider_item_id: String,
        output_json: Value,
        /// When this message fact occurred, in epoch milliseconds, when the
        /// provider exposes a per-message time (`None` otherwise). A neutral,
        /// optional message fact — carried verbatim, not parsed. Codex fills it
        /// from the item's `startedAtMs` / `completedAtMs`; Claude leaves it
        /// `None` (its `created_at` comes from the transcript fold).
        at_ms: Option<i64>,
    },
    /// The agent is asking for a permission decision.
    PermissionRequested { request: AgentPermissionRequest },
    /// A permission request was resolved with a decision.
    PermissionResolved {
        request_id: String,
        decision: PermissionDecision,
    },
    /// A server-to-client interaction Delta does not model yet. Surfacing it
    /// (rather than dropping or blocking on it) is the invariant that keeps an
    /// app-server session from silently hanging on an unhandled request.
    UnsupportedInteraction { method: String, detail_json: Value },
    /// The turn finished, with its terminal status.
    TurnCompleted { status: TurnStatus },
    /// An error occurred. `recoverable` distinguishes a transient error the
    /// session can continue past from a terminal one.
    Error { recoverable: bool, message: String },
}
