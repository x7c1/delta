use tokio::sync::oneshot;

use crate::error::Result;
use crate::interactor::hooks::ASK_USER_QUESTION;
use crate::interactor::permission_decision::PermissionDecision;
use crate::interactor::session_actor::actor::SessionContext;
use crate::interactor::session_actor::runtime::PendingPermission;
use crate::ports::{GitWorktree, SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace};

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

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Handle a `PermissionRequest` hook: an interactive permission dialog has
    /// actually appeared, so a human answer is genuinely pending. (Unlike
    /// `PreToolUse`, this never fires for auto-approved or classifier-handled
    /// calls.)
    ///
    /// The handler creates and owns the request row directly (the hook payload
    /// carries no `tool_use_id`, so the row records none) and registers a
    /// oneshot waiter for the browser's decision on this actor's state. The
    /// transport broadcasts the returned `PermissionRequested`, then blocks
    /// the hook response on the receiver with a deadline
    /// (`permission_decision_deadline`):
    ///
    /// - A browser decision (`decide_permission`) resolves the row and the
    ///   hook answers Claude Code with `hookSpecificOutput.decision`.
    /// - On timeout the transport abandons the waiter
    ///   (`abandon_permission_decision`) and responds with an empty
    ///   passthrough: Claude Code falls back to its interactive TUI prompt
    ///   exactly as before, the row stays `pending`, and the eventual
    ///   `tool_result` resolves it (see `sync_transcript`).
    pub(in crate::interactor) async fn on_permission_request(
        &mut self,
        tool_name: &str,
        tool_input_json: &str,
        transcript_path: &str,
    ) -> Result<PermissionWait> {
        // DIAGNOSTIC (to be reverted): permission dialogs are by definition
        // interesting, so log every PermissionRequest's `transcript_path` to
        // confirm whether the foreign-transcript filter ever catches a nested
        // subagent's dialog. (The hook payload carries no `tool_use_id`, so it
        // is not in the structured fields.) `tool_input_json` deliberately not
        // logged — content can be large or sensitive.
        tracing::info!(
            target: "delta_usecase::interactor::hooks::probe",
            session_id = %self.id,
            tool_name = %tool_name,
            transcript_path = %transcript_path,
            "PermissionRequest probe: received"
        );

        // A permission dialog raised by a nested subagent's tool call is
        // dispatched under the parent session's id but its `transcript_path`
        // points at the subagent's own JSONL. Short-circuit so the parent
        // session does not record a row, register a waiter, or broadcast a
        // notice for a dialog that is gated entirely inside the nested
        // subagent's TUI. The transport pattern matches the
        // `AskUserQuestion` short-circuit below: a dropped sender resolves
        // the receiver immediately, so the hook answers Claude Code with an
        // empty 200 and the dialog falls through to the TUI as normal.
        if self.is_foreign_transcript(transcript_path).await? {
            tracing::info!(
                target: "delta_usecase::interactor::hooks::probe",
                session_id = %self.id,
                tool_name = %tool_name,
                transcript_path = %transcript_path,
                "PermissionRequest probe: filtered as foreign transcript"
            );
            let (sender, receiver) = oneshot::channel();
            drop(sender);
            return Ok(PermissionWait {
                request_id: 0,
                decision: receiver,
                events: vec![],
            });
        }

        // Claude Code's `AskUserQuestion` is not a gateable action — a hook
        // cannot return the chosen option, and the question card is already
        // driven off `PreToolUse` (see `on_pre_tool_use`). So short-circuit
        // here to an immediate passthrough: no row, no waiter, no
        // `pending_permission`, no event. The `decision` receiver's sender is
        // dropped right away, so the transport's `timeout(...)` resolves at
        // once with a closed channel → empty passthrough → the TUI prompt
        // appears instantly (no 50s block, no duplicate Allow/Deny notice).
        if tool_name == ASK_USER_QUESTION {
            let (sender, receiver) = oneshot::channel();
            drop(sender);
            return Ok(PermissionWait {
                // No waiter is registered for this id, so the index entry it
                // seeds and the `abandon_permission_decision` that removes it
                // are both harmless no-ops (`take_permission_waiter` finds
                // nothing).
                request_id: 0,
                decision: receiver,
                events: vec![],
            });
        }

        let request = self
            .store
            .record_permission_request(self.id, tool_name, tool_input_json, None)
            .await?;

        let (sender, receiver) = oneshot::channel();
        self.state.insert_permission_waiter(request.id, sender);
        // Mirror the broadcast below into queryable runtime state, so a client
        // that misses the event (socket down) can rebuild the notice from the
        // sends envelope. Cleared on resolution or when the turn ends.
        self.state.set_pending_permission(PendingPermission {
            request_id: request.id,
            tool_name: tool_name.to_owned(),
            tool_input_json: tool_input_json.to_owned(),
        });

        Ok(PermissionWait {
            request_id: request.id,
            decision: receiver,
            events: vec![SessionEvent::PermissionRequested {
                session_id: self.id.clone(),
                request_id: request.id,
                tool_name: tool_name.to_owned(),
                tool_input_json: tool_input_json.to_owned(),
            }],
        })
    }
}
