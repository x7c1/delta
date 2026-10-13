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
use crate::ports::RateLimitWindow;

mod agent_file_change;
pub use agent_file_change::AgentFileChange;

mod agent_file_change_detail;
pub use agent_file_change_detail::AgentFileChangeDetail;

mod agent_file_change_kind;
pub use agent_file_change_kind::AgentFileChangeKind;

mod agent_permission_request;
pub use agent_permission_request::AgentPermissionRequest;

mod agent_token_usage;
pub use agent_token_usage::AgentTokenUsage;

mod session_end_reason;
pub use session_end_reason::SessionEndReason;

mod turn_status;
pub use turn_status::TurnStatus;

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
    /// A streaming fragment of the model's extended thinking (its internal
    /// reasoning, not its reply). Deliberately distinct from
    /// [`AgentEvent::AssistantDelta`] so reasoning is never mistaken for reply
    /// text. Claude never emits it (its thinking arrives already folded into the
    /// transcript's message content).
    ThinkingDelta {
        provider_item_id: String,
        text: String,
    },
    /// A completed block of the model's extended thinking, folded into a
    /// `Thinking` content block rather than reply text. Claude never emits it
    /// (its thinking arrives already folded into the transcript's message
    /// content).
    ThinkingMessage {
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
    /// The session's token accounting changed (a turn consumed context).
    ///
    /// Session-scoped and observability-only: it completes no message, touches
    /// no turn state, and is never persisted — it exists so the browser can show
    /// how full the context window is. A provider with no usage edge simply
    /// never emits it, and the display stays empty.
    TokenUsageUpdated { usage: AgentTokenUsage },
    /// The **account's** rate-limit windows changed, as observed on this
    /// session's provider.
    ///
    /// Account-scoped, not session-scoped: one account's limits are shared by
    /// every session of that provider (a single Codex app-server connection
    /// hosts many of them), so the adapter surfaces the same fact on each live
    /// session and the browser keys it by provider rather than by session.
    ///
    /// The windows replace the account's previous ones wholesale — an adapter
    /// whose provider sends *sparse* updates merges them against what it last
    /// observed before emitting, since only the adapter knows its provider's
    /// merge rules. An empty vector means "this account has no windows".
    RateLimitsUpdated { windows: Vec<RateLimitWindow> },
    /// The turn finished, with its terminal status.
    TurnCompleted { status: TurnStatus },
    /// An error occurred. `recoverable` distinguishes a transient error the
    /// session can continue past from a terminal one.
    Error { recoverable: bool, message: String },
}
