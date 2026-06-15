//! `PostToolUse` payload.

use serde::{Deserialize, Serialize};

/// `PostToolUse` payload.
///
/// Fires after a tool call completes, carrying the same `tool_use_id` the
/// matching `PreToolUse` carried, so Delta can correlate a subagent's end with
/// its start. `tool_response` is present in the real hook but Delta does not
/// need it (it only matches on the tool name and id), so it is not modelled.
#[derive(Debug, Deserialize, Serialize)]
pub struct PostToolUsePayload {
    pub session_id: String,
    pub tool_name: String,
    /// The id of the completed tool call, matching the `tool_use_id` of the
    /// `PreToolUse` that opened it.
    pub tool_use_id: String,
}
