//! `PostToolUse` payload.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `PostToolUse` payload.
///
/// Fires after a tool call completes, carrying the same `tool_use_id` the
/// matching `PreToolUse` carried, so Delta can correlate a subagent's end with
/// its start. `tool_response` is the structured tool result Claude Code writes
/// back into the transcript — for a background `Agent` launch it carries the
/// background task identifier (`agentId`), which Delta records as a fallback
/// correlation key on the running subagent.
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct PostToolUsePayload {
    pub session_id: String,
    pub tool_name: String,
    /// The id of the completed tool call, matching the `tool_use_id` of the
    /// `PreToolUse` that opened it.
    pub tool_use_id: String,
    /// The structured `tool_result` content the tool reported back. Optional
    /// because not every hook payload carries it (e.g. simulated test events,
    /// or a tool that returns no content); `Value::Null` and an empty object
    /// are both valid "nothing useful here" shapes that the consumer must
    /// tolerate. Inspected by the subagent handler for an `agentId` field on a
    /// background `Agent` launch.
    #[serde(default)]
    pub tool_response: Value,
    /// The JSONL the hook is firing against. For a nested subagent's tool call
    /// this is the subagent's own transcript, not the parent session's. The
    /// interactor compares this against the session row's stored path so a
    /// hook fired against a nested transcript can be filtered out — its
    /// `session_id` still names the parent but the tool call belongs to the
    /// nested subagent's transcript.
    pub transcript_path: String,
}
