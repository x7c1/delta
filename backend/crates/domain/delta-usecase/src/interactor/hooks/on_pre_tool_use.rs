use crate::error::Result;
use crate::interactor::hooks::ASK_USER_QUESTION;
use crate::interactor::session_actor::runtime::PendingQuestion;
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
    ///
    /// The one exception is Claude Code's built-in [`ASK_USER_QUESTION`] tool:
    /// it presents a multiple-choice question rather than a gateable action, so
    /// here — where the recorded row carries the `tool_use_id` that the later
    /// `tool_result` resolves it by — Delta also remembers the question and
    /// emits [`SessionEvent::QuestionAsked`] so the browser shows a dedicated
    /// question card. (Its sibling `PermissionRequest` hook passes straight
    /// through; see `on_permission_request`.)
    pub(in crate::interactor) async fn on_pre_tool_use(
        &mut self,
        tool_name: &str,
        tool_input_json: &str,
        tool_use_id: &str,
    ) -> Result<Vec<SessionEvent>> {
        let request = self
            .store
            .record_permission_request(self.id, tool_name, tool_input_json, Some(tool_use_id))
            .await?;

        if tool_name == ASK_USER_QUESTION {
            // Mirror the broadcast into queryable runtime state, so a client
            // that misses the event (socket down) rebuilds the question card
            // from the sends envelope. Cleared on resolution or turn end.
            self.state.set_pending_question(PendingQuestion {
                request_id: request.id,
                tool_input_json: tool_input_json.to_owned(),
            });
            return Ok(vec![SessionEvent::QuestionAsked {
                session_id: self.id.clone(),
                request_id: request.id,
                tool_input_json: tool_input_json.to_owned(),
            }]);
        }

        Ok(vec![])
    }
}
