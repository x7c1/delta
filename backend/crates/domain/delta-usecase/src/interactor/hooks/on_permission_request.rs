use crate::error::Result;
use crate::ports::{SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace};
use crate::Interactor;

impl<T, X, S, W> Interactor<T, X, S, W>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
{
    /// Handle a `PermissionRequest` hook: an interactive permission dialog has
    /// actually appeared, so a human answer is genuinely pending. Correlate it to
    /// the request row recorded at PreToolUse and emit `PermissionRequested` so the
    /// browser shows the notice. (Unlike PreToolUse, this never fires for
    /// auto-approved or classifier-handled calls.)
    pub async fn on_permission_request(
        &self,
        session_id: &delta_model::SessionId,
        tool_name: &str,
        tool_input_json: &str,
    ) -> Result<Vec<SessionEvent>> {
        match self
            .store
            .find_open_permission_request(session_id, tool_name, tool_input_json)
            .await?
        {
            Some(request_id) => Ok(vec![SessionEvent::PermissionRequested {
                session_id: session_id.clone(),
                request_id,
                tool_name: tool_name.to_owned(),
            }]),
            None => {
                tracing::warn!(
                    session_id = %session_id.as_str(),
                    tool_name,
                    "PermissionRequest with no matching recorded request; emitting no notice"
                );
                Ok(vec![])
            }
        }
    }
}
