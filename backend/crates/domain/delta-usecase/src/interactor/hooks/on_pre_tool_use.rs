use delta_attribution::claude_format;

use crate::error::Result;
use crate::interactor::hooks::{is_subagent_tool, ASK_USER_QUESTION};
use crate::interactor::session_actor::actor::SessionContext;
use crate::interactor::session_actor::runtime::{PendingQuestion, RunningSubagent};
use crate::ports::{GitWorktree, SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace};

/// Read an optional string field out of a tool-input JSON object.
///
/// Returns `None` when the input is not an object, the key is missing, the
/// value is not a string, or the string is empty — so a malformed or partial
/// `Agent` input degrades to "no label" rather than failing the hook.
fn string_field(tool_input_json: &str, key: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(tool_input_json)
        .ok()?
        .get(key)?
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
            // Attribute the question to the in-progress turn's thread, resolved
            // the same way the streaming preview resolves its thread (see
            // `on_message_display`): the in-flight send's thread, else the
            // latest persisted user message, else the session's main thread.
            // AskUserQuestion blocks synchronously within the turn, so this IS
            // the asking thread — the browser shows the card only there. The
            // in-flight-send step is what keeps a mid-turn branch question on
            // the new branch thread, whose user line is not yet ingested.
            let thread_id = self.store.in_progress_turn_thread(self.id).await?;
            // Mirror the broadcast into queryable runtime state, so a client
            // that misses the event (socket down) rebuilds the question card
            // from the sends envelope. Cleared on resolution or turn end.
            self.state.set_pending_question(PendingQuestion {
                request_id: request.id,
                thread_id,
                tool_input_json: tool_input_json.to_owned(),
            });
            return Ok(vec![SessionEvent::QuestionAsked {
                session_id: self.id.clone(),
                request_id: request.id,
                thread_id,
                tool_input_json: tool_input_json.to_owned(),
            }]);
        }

        if is_subagent_tool(tool_name) {
            // A subagent (the `Agent`/`Task` tool) is starting. It runs in its
            // own transcript that Delta never tails, so the conversation pane
            // would otherwise show nothing while it works — track it as running
            // and broadcast the start. Keyed by `tool_use_id`.
            //
            // A FOREGROUND subagent is cleared by the matching `PostToolUse`
            // (which fires when it completes) and swept at turn end. A BACKGROUND
            // subagent (`run_in_background: true`) returns immediately — its
            // `PostToolUse` fires at launch, not completion — so it must survive
            // both the immediate `PostToolUse` and the turn-end sweep, finishing
            // only when its completion `<task-notification>` is folded. The
            // `background` flag, read from the same top-level `run_in_background`
            // key the attribution fold keys on, drives that distinction. The
            // runtime mirror lets a client that missed the event rebuild its
            // indicator from the sends envelope.
            let subagent_type = string_field(tool_input_json, "subagent_type");
            let description = string_field(tool_input_json, "description");
            let background = serde_json::from_str::<serde_json::Value>(tool_input_json)
                .as_ref()
                .map(claude_format::launches_in_background)
                .unwrap_or(false);
            let newly = self.state.start_subagent(RunningSubagent {
                tool_use_id: tool_use_id.to_owned(),
                subagent_type: subagent_type.clone(),
                description: description.clone(),
                background,
            });
            // A duplicate `PreToolUse` for an already-tracked id (a retried hook
            // delivery) must not double-broadcast, so only emit on a new entry.
            if newly {
                return Ok(vec![SessionEvent::SubagentStarted {
                    session_id: self.id.clone(),
                    tool_use_id: tool_use_id.to_owned(),
                    subagent_type,
                    description,
                    background,
                }]);
            }
        }

        Ok(vec![])
    }
}
