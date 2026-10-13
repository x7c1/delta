//! A tool-permission request surfaced by the agent, in provider-neutral form.

use serde_json::Value;

use super::AgentFileChangeDetail;

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
