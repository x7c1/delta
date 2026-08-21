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

/// Token accounting for one session, as the provider's own edge reported it.
///
/// Observability only — nothing here is persisted or fed to the turn machine.
/// The fields mirror [`crate::ports::StatusSnapshot`]'s context fields so the
/// pump can forward them without inventing anything.
///
/// **The percentage is the provider's to compute, never the core's** — only the
/// provider knows what window the counts are against (see
/// [`crate::ports::StatusSnapshot`] for what each edge does with that rule).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AgentTokenUsage {
    /// Percentage of the context window in use, or `None` when the provider
    /// cannot say (the window size is unknown). Never a placeholder — an absent
    /// percentage hides the bar rather than pinning it at 0%.
    pub context_used_percentage: Option<f64>,
    /// The context window's total size in tokens.
    pub context_window_size: Option<u64>,
    /// Tokens currently occupying the context window.
    pub context_current_usage: Option<u64>,
    /// Total input tokens sent this session.
    pub total_input_tokens: Option<u64>,
}

/// How one file would change if a permission request is allowed.
///
/// Deliberately a small, closed vocabulary rather than a passthrough of any one
/// provider's spelling: these three are what a patch can do to a path, and a
/// request that names something else degrades to no kind at all (see
/// [`AgentFileChange::kind`]) instead of inventing one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentFileChangeKind {
    /// The file does not exist yet and would be created.
    Add,
    /// An existing file's contents would be edited.
    Update,
    /// The file would be removed.
    Delete,
}

/// One file a permission request would change, in provider-neutral form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentFileChange {
    /// The path that would change, exactly as the provider named it.
    pub path: String,
    /// How it would change, or `None` when the provider named a kind this build
    /// does not model — the path and the diff are still worth showing, so an
    /// unknown kind loses the label rather than the whole entry.
    pub kind: Option<AgentFileChangeKind>,
    /// The unified diff of the proposed change, as the provider produced it.
    pub diff: String,
}

/// What a permission request would do to files on disk, when the provider says
/// so before asking.
///
/// This is *display* detail for a live prompt: it exists so the approval card
/// can name the files it is gating and show the diff, instead of a truncated
/// blob of request params. It is `Some` only when the provider actually stated
/// the change set — the adapter never synthesises an empty detail, so a `None`
/// means "nothing is known here" and the card falls back to its input summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentFileChangeDetail {
    /// The files the request would change, in the order the provider listed
    /// them.
    pub changes: Vec<AgentFileChange>,
    /// The provider's own explanation for why it is asking (Codex's `reason`,
    /// e.g. a request for write access outside the sandbox). `None` when the
    /// provider offered none.
    pub reason: Option<String>,
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
    /// What the request would do to files on disk, when the provider stated it
    /// (see [`AgentFileChangeDetail`]). `None` for every request that is not a
    /// file change, and for a file change whose detail the adapter could not
    /// correlate — both render from [`Self::input_json`] alone, exactly as they
    /// did before this existed.
    pub file_change: Option<AgentFileChangeDetail>,
    /// A directory the request *also* asks to be allowed to write under for the
    /// remainder of the session, when the provider asked for one (Codex's
    /// `grantRoot`). `None` whenever it asked for no such root.
    ///
    /// A sibling of [`Self::file_change`], deliberately not a part of it, for
    /// two reasons. It is a **broader** ask — the detail names the files this
    /// one request would touch, this asks for a standing permission over a
    /// whole tree — and it arrives on the request's own params rather than from
    /// a correlated item, so nesting it would hide it in exactly the case where
    /// the card has least else to go on: a change set that could not be
    /// correlated.
    pub grant_root: Option<String>,
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
