use tokio::sync::oneshot;

use crate::error::Result;
use crate::interactor::permission_decision::PermissionDecision;
use crate::ports::{SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace};
use crate::Interactor;

/// What `on_permission_request` hands the transport: the request row's id, a
/// receiver the transport awaits (with its own deadline) for the browser's
/// decision, and the `PermissionRequested` event to broadcast *before*
/// blocking — otherwise the browser could never see the notice it is supposed
/// to answer.
pub struct PermissionWait {
    pub request_id: i64,
    pub decision: oneshot::Receiver<PermissionDecision>,
    pub events: Vec<SessionEvent>,
}

impl<T, X, S, W> Interactor<T, X, S, W>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
{
    /// Handle a `PermissionRequest` hook: an interactive permission dialog has
    /// actually appeared, so a human answer is genuinely pending. (Unlike
    /// `PreToolUse`, this never fires for auto-approved or classifier-handled
    /// calls.)
    ///
    /// The handler creates and owns the request row directly (the hook payload
    /// carries no `tool_use_id`, so the row records none) and registers a
    /// oneshot waiter for the browser's decision. The transport broadcasts the
    /// returned `PermissionRequested`, then blocks the hook response on the
    /// receiver with a deadline ([`Self::permission_decision_deadline`]):
    ///
    /// - A browser decision (`decide_permission`) resolves the row and the
    ///   hook answers Claude Code with `hookSpecificOutput.decision`.
    /// - On timeout the transport abandons the waiter
    ///   (`abandon_permission_decision`) and responds with an empty
    ///   passthrough: Claude Code falls back to its interactive TUI prompt
    ///   exactly as before, the row stays `pending`, and the eventual
    ///   `tool_result` resolves it (see `sync_transcript`).
    pub async fn on_permission_request(
        &self,
        session_id: &delta_model::SessionId,
        tool_name: &str,
        tool_input_json: &str,
    ) -> Result<PermissionWait> {
        let request = self
            .store
            .record_permission_request(session_id, tool_name, tool_input_json, None)
            .await?;

        let (sender, receiver) = oneshot::channel();
        self.pending_permissions
            .lock()
            .await
            .insert(request.id, sender);

        Ok(PermissionWait {
            request_id: request.id,
            decision: receiver,
            events: vec![SessionEvent::PermissionRequested {
                session_id: session_id.clone(),
                request_id: request.id,
                tool_name: tool_name.to_owned(),
                tool_input_json: tool_input_json.to_owned(),
            }],
        })
    }
}
