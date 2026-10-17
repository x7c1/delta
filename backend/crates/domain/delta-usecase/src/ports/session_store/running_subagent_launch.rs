//! One outstanding launch that lit the running-subagent indicator.

use delta_model::ThreadId;

/// An outstanding background launch that lit the running-subagent indicator,
/// as the `subagent_launch` store keeps it: everything a re-adopted session
/// needs to light that indicator again for a subagent that is still running in
/// the surviving pane, after the restart forgot the in-memory running set.
///
/// Only launches the transcript fold lit the indicator for are returned —
/// a background `Agent`/`Task` call and a forked skill. A background `Bash`
/// also leaves an outstanding launch (for attributing its completion), but it
/// never lit the indicator, so it never reads back as one of these.
///
/// Every such launch is a BACKGROUND one: a foreground subagent leaves no
/// launch row at all (it cannot outlive its turn), so there is no `background`
/// field to carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningSubagentLaunch {
    /// The launching `tool_use_id`, or the synthetic `forked-skill:<agentId>`
    /// of a forked skill.
    pub tool_use_id: String,
    /// The thread the launch line was attributed to.
    pub thread_id: ThreadId,
    /// The background-task id, once it is known (see
    /// [`SubagentLaunchStore::upgrade_subagent_task_id`]).
    ///
    /// [`SubagentLaunchStore::upgrade_subagent_task_id`]: super::SubagentLaunchStore::upgrade_subagent_task_id
    pub task_id: Option<String>,
    /// The subagent type the launch carried (a forked skill's skill name), if
    /// any. A launch recorded before the store kept it reads `None`.
    pub subagent_type: Option<String>,
    /// The launch's short task description, if any. A launch recorded before
    /// the store kept it reads `None`.
    pub description: Option<String>,
}
