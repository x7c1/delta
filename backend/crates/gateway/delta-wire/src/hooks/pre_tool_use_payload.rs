//! `PreToolUse` payload.

use serde::{Deserialize, Serialize};

/// `PreToolUse` payload.
#[derive(Debug, Deserialize, Serialize)]
pub struct PreToolUsePayload {
    pub session_id: String,
    pub tool_name: String,
    #[serde(default)]
    pub tool_input: serde_json::Value,
    /// The id of the imminent tool call, e.g. `"toolu_0166..."`. It is the exact
    /// key Claude Code later writes as `tool_use_id` on the matching
    /// `tool_result` transcript line, so Delta can correlate the recorded
    /// permission request with its completion and auto-clear the notice.
    pub tool_use_id: String,
    /// The JSONL the hook is firing against. For a nested subagent's tool call
    /// this is the subagent's own transcript (e.g.
    /// `<parent-session>/subagents/agent-<id>.jsonl`), not the parent session's
    /// `<parent-session>.jsonl`. The interactor compares this against the
    /// session row's stored path so a hook fired against a nested transcript
    /// can be filtered out — its `session_id` still names the parent (Claude
    /// Code dispatches hooks under the parent's id) but the runtime work
    /// belongs to a different conversation Delta does not track.
    pub transcript_path: String,
}
