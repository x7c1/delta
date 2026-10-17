//! Outstanding background-task launches keyed by their launching tool_use id.

use std::collections::BTreeMap;

use async_trait::async_trait;

use delta_attribution::SubagentLaunch;
use delta_model::{SessionId, ThreadId};

use super::{RunningSubagentLaunch, SessionStore};
use crate::error::Result;

/// The outstanding background-task launches, keyed by the launching tool_use
/// id, that let a late `<task-notification>` find its launching thread, and,
/// for the launches that lit the running-subagent indicator, let a re-adopted
/// session light it again (see [`Self::running_subagent_launches`]).
///
/// One of the concerns [`SessionStore`] gathers.
#[async_trait]
pub trait SubagentLaunchStore: Send + Sync {
    /// Record (or refresh) the launching thread of a background task, keyed by
    /// the launching tool_use `id`. A background `Agent`/`Task`/`Bash`
    /// (`run_in_background: true`) returns immediately and its completion is
    /// injected later — frequently in a different sync window — as a
    /// `<task-notification>` carrying this same id. Persisting `(tool_use_id ->
    /// thread_id)` lets [`Self::outstanding_subagent_launches`] reseed the
    /// attribution fold so the notification is attributed to the launching
    /// thread rather than whatever thread is current when it lands. Idempotent:
    /// re-recording the same id is a no-op refresh (the row's `task_id`, if
    /// previously set, is preserved — see [`Self::upgrade_subagent_task_id`]).
    async fn record_subagent_launch(
        &self,
        session_id: &SessionId,
        tool_use_id: &str,
        thread_id: ThreadId,
    ) -> Result<()>;

    /// Upgrade a recorded background-task launch with the `task_id` Claude Code
    /// minted for the subagent. The id is learned later than the launch itself:
    /// the `PostToolUse(Agent)` hook reads it from the launching tool's
    /// `tool_result` (the `agentId` field) and persists it here so a subsequent
    /// `<task-notification>` whose `<tool-use-id>` element was stripped can
    /// still be matched by its `<task-id>` element. Idempotent: re-upgrading
    /// the same id with the same `task_id` is a no-op refresh. Upgrading an
    /// unknown launch is also a no-op (the launch may have been folded already).
    async fn upgrade_subagent_task_id(
        &self,
        session_id: &SessionId,
        tool_use_id: &str,
        task_id: &str,
    ) -> Result<()>;

    /// Clear a recorded background-task launch once its `<task-notification>`
    /// has been folded. Clearing an unknown id is a no-op.
    async fn clear_subagent_launch(&self, session_id: &SessionId, tool_use_id: &str) -> Result<()>;

    /// The session's outstanding background-task launches as `(tool_use_id ->
    /// SubagentLaunch)`: every background task still awaiting its
    /// `<task-notification>`, each entry pairing the launching thread with the
    /// optional `task_id` learned via `PostToolUse(Agent)`. Seeds the
    /// attribution fold at sync start so a completion landing in a later
    /// window finds its launching thread by either correlation key.
    async fn outstanding_subagent_launches(
        &self,
        session_id: &SessionId,
    ) -> Result<BTreeMap<String, SubagentLaunch>>;

    /// Mark a recorded background launch as one that lit the running-subagent
    /// indicator, with the `subagent_type` and `description` the indicator
    /// shows. Called when the transcript fold starts the indicator for a
    /// BACKGROUND launch, right after [`Self::record_subagent_launch`] recorded
    /// its row in the same fold, so [`Self::running_subagent_launches`] can
    /// light it again for a re-adopted pane whose launch line is already
    /// behind the cursor. Marking an unknown launch is a no-op.
    async fn record_subagent_indicator(
        &self,
        session_id: &SessionId,
        tool_use_id: &str,
        subagent_type: Option<&str>,
        description: Option<&str>,
    ) -> Result<()>;

    /// The session's outstanding launches that lit the running-subagent
    /// indicator (see [`Self::record_subagent_indicator`]), oldest first —
    /// the running set a re-adopted session rebuilds. A launch that never lit
    /// it (a background `Bash`) is not among them.
    async fn running_subagent_launches(
        &self,
        session_id: &SessionId,
    ) -> Result<Vec<RunningSubagentLaunch>>;
}

#[async_trait]
impl SubagentLaunchStore for Box<dyn SessionStore> {
    async fn record_subagent_launch(
        &self,
        session_id: &SessionId,
        tool_use_id: &str,
        thread_id: ThreadId,
    ) -> Result<()> {
        (**self)
            .record_subagent_launch(session_id, tool_use_id, thread_id)
            .await
    }

    async fn upgrade_subagent_task_id(
        &self,
        session_id: &SessionId,
        tool_use_id: &str,
        task_id: &str,
    ) -> Result<()> {
        (**self)
            .upgrade_subagent_task_id(session_id, tool_use_id, task_id)
            .await
    }

    async fn clear_subagent_launch(&self, session_id: &SessionId, tool_use_id: &str) -> Result<()> {
        (**self)
            .clear_subagent_launch(session_id, tool_use_id)
            .await
    }

    async fn outstanding_subagent_launches(
        &self,
        session_id: &SessionId,
    ) -> Result<BTreeMap<String, SubagentLaunch>> {
        (**self).outstanding_subagent_launches(session_id).await
    }

    async fn record_subagent_indicator(
        &self,
        session_id: &SessionId,
        tool_use_id: &str,
        subagent_type: Option<&str>,
        description: Option<&str>,
    ) -> Result<()> {
        (**self)
            .record_subagent_indicator(session_id, tool_use_id, subagent_type, description)
            .await
    }

    async fn running_subagent_launches(
        &self,
        session_id: &SessionId,
    ) -> Result<Vec<RunningSubagentLaunch>> {
        (**self).running_subagent_launches(session_id).await
    }
}
