use crate::error::Result;
use crate::interactor::hooks::is_subagent_tool;
use crate::interactor::session_actor::actor::SessionContext;
use crate::ports::{GitWorktree, SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace};

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Handle a `PostToolUse` hook: the only thing Delta does with it is close
    /// a subagent's running window.
    ///
    /// `PostToolUse` fires for every tool call (including a subagent's own
    /// nested tools), so it is matched strictly against the subagent tool names
    /// (`{Agent, Task}`) — a nested `Bash` carries its own name and so never
    /// clears the indicator. When a FOREGROUND subagent (`Agent`/`Task`)
    /// completes, the running entry recorded by the matching `PreToolUse` is
    /// removed by `tool_use_id` and a [`SessionEvent::SubagentFinished`] is
    /// broadcast.
    ///
    /// This is the FOREGROUND (synchronous) end signal only. A BACKGROUND
    /// subagent (`run_in_background: true`) gets its `PostToolUse` immediately at
    /// launch — the call returned, the subagent did not — so this must NOT
    /// finish it. [`SessionRuntime::finish_foreground_subagent`] skips a
    /// background entry; it is finished later when its completion
    /// `<task-notification>` is folded during transcript sync.
    ///
    /// A `PostToolUse` whose `tool_use_id` is not tracked (an unknown id, one
    /// already cleared when the turn ended, or a still-running background entry)
    /// is a no-op: nothing is removed and nothing is broadcast, so a stray or
    /// background end never produces a spurious event.
    pub(in crate::interactor) async fn on_post_tool_use(
        &mut self,
        tool_name: &str,
        tool_use_id: &str,
    ) -> Result<Vec<SessionEvent>> {
        if !is_subagent_tool(tool_name) {
            return Ok(vec![]);
        }

        if self.state.finish_foreground_subagent(tool_use_id) {
            return Ok(vec![SessionEvent::SubagentFinished {
                session_id: self.id.clone(),
                tool_use_id: tool_use_id.to_owned(),
            }]);
        }

        Ok(vec![])
    }
}
