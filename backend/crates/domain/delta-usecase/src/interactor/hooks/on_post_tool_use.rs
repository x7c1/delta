use delta_attribution::claude_format;

use crate::error::Result;
use crate::interactor::hooks::{is_subagent_tool, HookTranscript};
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
fn agent_id_field(tool_response: &serde_json::Value) -> Option<String> {
    tool_response
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
    /// must NOT finish the running entry.
    /// [`SessionRuntime::finish_foreground_subagent`](crate::interactor::session_actor::runtime::SessionRuntime::finish_foreground_subagent)
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
    /// The `agentId` is also stashed via
    /// `SessionRuntime::record_pending_post_tool_use_agent_id` before the direct
    /// upgrade runs, so a `PostToolUse` that arrives before the parent's
    /// `tool_use(Agent)` line has been ingested (the race introduced when the
    /// indicator moved off the hook path) still gets applied when the sync
    /// later lights the indicator.
    ///
    /// A subagent recorded as BACKGROUND that Claude Code nevertheless ran in
    /// the foreground (the tool_use carries no `run_in_background` key, and
    /// Claude Code decides per subagent type) reports `"status": "completed"`
    /// in its `tool_response` instead of `"async_launched"`. No
    /// `<task-notification>` follows such a run, so the entry is finished here,
    /// its launch row cleared, and [`SessionEvent::SubagentFinished`]
    /// broadcast.
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
        // A nested subagent's PostToolUse is dispatched under the parent
        // session's id but its `transcript_path` points at the subagent's
        // own JSONL. Ignore it so a nested completion cannot clear (or
        // upgrade) a parent-tracked running entry that happens to share the
        // same `tool_use_id` by accident, and so the symmetric `PreToolUse`
        // no-op (above) is not contradicted later. A hook naming the session's
        // own transcript at a new path (Claude Code moved it) re-points the
        // session and is handled as usual.
        if self.admit_hook_transcript(transcript_path).await? == HookTranscript::Foreign {
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

        // A malformed response degrades to "no information" (`null`): no
        // foreground-run finish and no `agentId` upgrade, never a failed hook.
        let tool_response = serde_json::from_str::<serde_json::Value>(tool_response_json)
            .unwrap_or(serde_json::Value::Null);

        // Recorded as background but run in the foreground (see the doc
        // above). The fold reaches the same verdict from the `tool_result`
        // text (cold-start replay, or a hook that never arrived); finishing is
        // idempotent, so whichever comes second is a no-op.
        if claude_format::subagent_ran_in_foreground_by_tool_response(&tool_response)
            && self.state.finish_subagent(tool_use_id)
        {
            self.store
                .clear_subagent_launch(self.id, tool_use_id)
                .await?;
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
        //
        // The `PostToolUse(Agent)` for a top-level background launch can arrive
        // BEFORE `sync_transcript` has folded the assistant's `tool_use(Agent)`
        // line and lit the running entry: `PreToolUse(Agent)` force-syncs the
        // parent transcript but the line is not always flushed to the JSONL by
        // the time the hook handler reads it. If the in-memory entry does not
        // yet exist the direct upgrade is a no-op and the `agentId` would be
        // lost. Stash it in `pending_post_tool_use_agent_ids` first so the
        // eventual `Effect::SubagentIndicatorStarted` arm of the sync can fold
        // it onto the freshly-created entry and persist it through the store.
        // When the in-memory entry IS already present the direct upgrade still
        // wins; the buffer entry is then harmless dead weight (the sync drain
        // finds a runtime value already set).
        if let Some(task_id) = agent_id_field(&tool_response) {
            self.state
                .record_pending_post_tool_use_agent_id(tool_use_id, &task_id);
            if self.state.upgrade_subagent_task_id(tool_use_id, &task_id) {
                self.store
                    .upgrade_subagent_task_id(self.id, tool_use_id, &task_id)
                    .await?;
            }
        }

        Ok(vec![])
    }
}
