//! [`SubagentLaunch`]: one outstanding background-task launch.

use delta_model::ThreadId;

/// One outstanding background-task launch: the launching thread of the task,
/// plus the [`task_id`] learned from the launching tool's `tool_result` once
/// the `PostToolUse(Agent)` hook ran. The map [`AttributionState::launched_threads`]
/// keys these by the launching tool_use id.
///
/// [`task_id`]: SubagentLaunch::task_id
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentLaunch {
    /// The thread the launching `Agent`/`Task`/`Bash` tool_use was attributed
    /// to. A completion `<task-notification>` carrying this launch's id (or
    /// matching `task_id`) is attributed back to this thread.
    pub thread_id: ThreadId,
    /// The background-task identifier the launching tool's `tool_result`
    /// reported, learned via the `PostToolUse(Agent)` hook. `None` until that
    /// hook has run, or when the upgrade was never persisted (an older row).
    /// Recorded so that a `<task-notification>` whose `<tool-use-id>` element
    /// was stripped can still be matched by its `<task-id>` element.
    pub task_id: Option<String>,
}
