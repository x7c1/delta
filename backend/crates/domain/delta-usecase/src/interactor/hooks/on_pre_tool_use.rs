use crate::error::Result;
use crate::interactor::session_actor::actor::SessionContext;
use crate::ports::{SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace};

impl<T, X, S, W> SessionContext<'_, T, X, S, W>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
{
    /// Handle a `PreToolUse` hook: only RECORD the permission request. This hook
    /// fires for every tool call (including auto-approved and long-running ones),
    /// so it is not a reliable signal that a human answer is pending — recording
    /// the request here carries the `tool_use_id` needed to resolve it once the
    /// matching `tool_result` is later ingested. The browser notice is emitted by
    /// the `PermissionRequest` hook instead, which fires only for genuine
    /// interactive prompts (and owns its own row plus the Allow/Deny wait —
    /// see `on_permission_request`). PreToolUse itself never returns
    /// allow/deny. Routed through the session's mailbox so the record is
    /// ordered before any ingest that could resolve it.
    pub(in crate::interactor) async fn on_pre_tool_use(
        &mut self,
        tool_name: &str,
        tool_input_json: &str,
        tool_use_id: &str,
    ) -> Result<Vec<SessionEvent>> {
        self.store
            .record_permission_request(self.id, tool_name, tool_input_json, Some(tool_use_id))
            .await?;
        Ok(vec![])
    }
}
