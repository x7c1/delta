use crate::error::Result;
use crate::interactor::hooks::is_subagent_tool;
use crate::interactor::session_actor::actor::SessionContext;
use crate::ports::{GitWorktree, SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace};

/// Pull the background-task identifier (`agentId`) out of the `tool_result`
/// content a `PostToolUse(Agent)` hook reports.
///
/// Claude Code reports a background `Agent`/`Task` launch's id under the
/// `agentId` key inside the tool's `tool_result`. Recent versions sometimes
/// drop the `<tool-use-id>` element from the eventual `<task-notification>`
/// body — `<task-id>` is all that survives — so capturing the id here gives
/// the notification a fallback correlation key. Returns `None` when the
/// response is not an object, the key is missing, or the value is not a
/// non-empty string, so a malformed/partial response degrades to "no upgrade"
/// rather than failing the hook.
fn agent_id_field(tool_response_json: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(tool_response_json)
        .ok()?
        .get("agentId")?
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Handle a `PostToolUse` hook: the only thing Delta does with it is close
    /// a subagent's running window — for a foreground subagent — or record the
    /// subagent's `agentId` against the launch row for a background subagent.
    ///
    /// `PostToolUse` fires for every tool call (including a subagent's own
    /// nested tools), so it is matched strictly against the subagent tool names
    /// (`{Agent, Task}`) — a nested `Bash` carries its own name and so never
    /// clears the indicator. When a FOREGROUND subagent (`Agent`/`Task`)
    /// completes, the running entry recorded by the matching `PreToolUse` is
    /// removed by `tool_use_id` and a [`SessionEvent::SubagentFinished`] is
    /// broadcast.
    ///
    /// For a BACKGROUND subagent (`run_in_background: true`) this hook fires
    /// immediately at launch — the call returned, the subagent did not — so it
    /// must NOT finish the running entry. [`SessionRuntime::finish_foreground_subagent`]
    /// skips a background entry; the indicator is cleared later when its
    /// completion `<task-notification>` is folded. But the launch's `tool_result`
    /// DOES land here, and it carries the `agentId` Claude Code minted for the
    /// subagent. Recent Claude Code versions sometimes drop `<tool-use-id>`
    /// from the user-message `<task-notification>` body while keeping
    /// `<task-id>`, so the handler records `agentId` against the matching
    /// running entry (and persists it via the launch row) to give the
    /// eventual notification a fallback correlation key. No event is
    /// broadcast for the upgrade — `SubagentStarted`/`SubagentFinished`
    /// already bracket the indicator, and the task id is a server-internal
    /// matching detail.
    ///
    /// A `PostToolUse` whose `tool_use_id` is not tracked (an unknown id, one
    /// already cleared when the turn ended, or a still-running background entry
    /// for which the upgrade lookup also misses) is a no-op: nothing is removed
    /// and nothing is broadcast, so a stray end never produces a spurious event.
    pub(in crate::interactor) async fn on_post_tool_use(
        &mut self,
        tool_name: &str,
        tool_use_id: &str,
        tool_response_json: &str,
        transcript_path: &str,
    ) -> Result<Vec<SessionEvent>> {
        // DIAGNOSTIC (to be reverted): mirror of the PreToolUse probe — log the
        // `transcript_path` carried by a PostToolUse for an `Agent`/`Task`
        // call. Helps confirm whether Pre and Post agree on the path for a
        // nested launch, since PostToolUse for a background launch fires
        // immediately at dispatch time.
        if is_subagent_tool(tool_name) {
            tracing::info!(
                target: "delta_usecase::interactor::hooks::probe",
                session_id = %self.id,
                tool_name = %tool_name,
                tool_use_id = %tool_use_id,
                transcript_path = %transcript_path,
                "PostToolUse probe: Agent/Task completion received"
            );
        }

        // A nested subagent's PostToolUse is dispatched under the parent
        // session's id but its `transcript_path` points at the subagent's
        // own JSONL. Ignore it so a nested completion cannot clear (or
        // upgrade) a parent-tracked running entry that happens to share the
        // same `tool_use_id` by accident, and so the symmetric `PreToolUse`
        // no-op (above) is not contradicted later.
        if self.is_foreign_transcript(transcript_path).await? {
            if is_subagent_tool(tool_name) {
                tracing::info!(
                    target: "delta_usecase::interactor::hooks::probe",
                    session_id = %self.id,
                    tool_name = %tool_name,
                    tool_use_id = %tool_use_id,
                    transcript_path = %transcript_path,
                    "PostToolUse probe: filtered as foreign transcript"
                );
            }
            return Ok(vec![]);
        }

        if !is_subagent_tool(tool_name) {
            return Ok(vec![]);
        }

        // Foreground end: clear the running entry and broadcast SubagentFinished.
        // `finish_foreground_subagent` is keyed and kind-aware, so a background
        // entry stays put and falls through to the upgrade branch below.
        if self.state.finish_foreground_subagent(tool_use_id) {
            return Ok(vec![SessionEvent::SubagentFinished {
                session_id: self.id.clone(),
                tool_use_id: tool_use_id.to_owned(),
            }]);
        }

        // Background launch ack: the call returned with the subagent's `agentId`
        // in its `tool_result`. Record it against the still-running entry so a
        // later `<task-notification>` missing `<tool-use-id>` can still finish
        // the subagent by `<task-id>`. The upgrade is best-effort — an absent
        // or malformed `agentId` simply leaves the entry as-is — and never
        // emits an event (the indicator hasn't changed; only the matching
        // metadata did).
        if let Some(task_id) = agent_id_field(tool_response_json) {
            if self.state.upgrade_subagent_task_id(tool_use_id, &task_id) {
                self.store
                    .upgrade_subagent_task_id(self.id, tool_use_id, &task_id)
                    .await?;
            }
        }

        Ok(vec![])
    }
}
