//! A subagent currently running for the session.

use delta_usecase::RunningSubagent;
use serde::Serialize;
use ts_rs::TS;

/// A subagent currently running for the session, as reported on the REST
/// surface: an `Agent`/`Task` tool call inside a turn, or the background agent
/// Claude Code forks for a slash command's skill, which runs with no turn in
/// flight at all.
///
/// The queryable counterpart of the `subagent_started` event (minus the session
/// id the URL already names): the start/finish events are lost for a client
/// whose socket was down when they fired, so a reconnecting client rebuilds its
/// running-subagent indicator from this list instead.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "RunningSubagent")]
pub struct WireRunningSubagent {
    /// The thread that launched the subagent, so a reconnecting client can keep
    /// that thread's running indicator lit — and its unread badge suppressed —
    /// until the subagent finishes, which for a background subagent outlives the
    /// launching turn.
    pub thread_id: i64,
    /// The launch's `tool_use_id`, its stable key — synthetic
    /// (`forked-skill:<agentId>`) for a forked skill, which makes no tool call.
    pub tool_use_id: String,
    /// The subagent type (e.g. `general-purpose`), a forked skill's skill name,
    /// or null if the launch carried none.
    pub subagent_type: Option<String>,
    /// The short task description, if the launch carried one, for display.
    pub description: Option<String>,
    /// Whether the launch runs in the background — `run_in_background: true` for
    /// a tool call, and always true for a forked skill. A reconnecting client
    /// carries this so its turn-end sweep keeps a surviving background subagent
    /// while dropping foreground ones.
    pub background: bool,
}

impl From<RunningSubagent> for WireRunningSubagent {
    fn from(subagent: RunningSubagent) -> Self {
        WireRunningSubagent {
            thread_id: subagent.thread_id.0,
            tool_use_id: subagent.tool_use_id,
            subagent_type: subagent.subagent_type,
            description: subagent.description,
            background: subagent.background,
        }
    }
}
