use tokio::sync::oneshot;

use crate::agent::{AgentEvent, AgentPermissionRequest};
use crate::error::Result;
use crate::interactor::agent_permission::reduce_permission_event;
use crate::interactor::hooks::ASK_USER_QUESTION;
use crate::interactor::permission_decision::PermissionDecision;
use crate::interactor::session_actor::actor::SessionContext;
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

        // The oneshot and the request-id → session index are Delta-internal
        // correlation/transport plumbing (they carry the browser decision back
        // to the blocked hook), so they stay here rather than in the neutral
        // event: the reducer only owns the core-loop effect.
        let (sender, receiver) = oneshot::channel();
        self.state.insert_permission_waiter(request.id, sender);

        // Express the request as the provider-neutral fact and let the
        // permission reducer raise the queryable mirror and produce the notice
        // broadcast. The `tool_use_id` is `None`: the `PermissionRequest` hook
        // payload carries none (the row records none too). `file_change` and
        // `grant_root` are `None` for the same reason — the hook states the tool
        // and its input and nothing about a proposed patch or a write root, so
        // the notice renders from the input alone, exactly as it always has.
        let event = AgentEvent::PermissionRequested {
            request: AgentPermissionRequest {
                request_id: request.id.to_string(),
                tool_name: tool_name.to_owned(),
                input_json: parse_tool_input(tool_input_json),
                tool_use_id: None,
                file_change: None,
                grant_root: None,
            },
        };
        let events = reduce_permission_event(self.state, self.id, &event);

        Ok(PermissionWait {
            request_id: request.id,
            decision: receiver,
            events,
        })
    }
}

/// Parse the hook's tool-input JSON text into the structured value the neutral
/// event carries. Delta's hook boundary already deserialised the payload
/// through `serde_json::Value`, so this text is well-formed JSON in production;
/// the fallback keeps the function total for any hand-built caller.
fn parse_tool_input(tool_input_json: &str) -> serde_json::Value {
    serde_json::from_str(tool_input_json).unwrap_or(serde_json::Value::Null)
}
