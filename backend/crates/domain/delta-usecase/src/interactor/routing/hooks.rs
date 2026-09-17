//! Hook deliveries: each Claude Code hook payload routed to the session the
//! hook names, spawning its actor on first contact — which is what registers
//! an externally-started session.

use delta_model::SessionId;

use crate::error::Result;
use crate::interactor::hooks::PermissionWait;
use crate::interactor::session_actor::input::SessionInput;
use crate::interactor::Interactor;
use crate::ports::{
    GitWorktree, MessageDisplayHook, SessionEndHook, SessionEvent, SessionStartHook, SessionStore,
    StopHook, TmuxDriver, Transcript, UserPromptSubmitHook, Workspace,
};

impl<T, X, S, W, G> Interactor<T, X, S, W, G>
where
    T: TmuxDriver + 'static,
    X: Transcript + 'static,
    S: SessionStore + 'static,
    W: Workspace + 'static,
    G: GitWorktree + 'static,
{
    /// Handle a `UserPromptSubmit` hook, routed to the session the hook names
    /// (spawning its actor on first contact, which is what registers an
    /// external session). Returns the events to broadcast and, when a locator
    /// quote should be injected, the `additionalContext` string.
    pub async fn on_user_prompt_submit(
        &self,
        hook: UserPromptSubmitHook,
    ) -> Result<(Vec<SessionEvent>, Option<String>)> {
        let id = hook.session_id.clone();
        self.request(&id, move |reply| SessionInput::UserPromptSubmit {
            hook,
            reply,
        })
        .await
    }

    /// Handle a `Stop` hook: ingest the final transcript lines and report the
    /// turn as completed.
    pub async fn on_stop(&self, hook: StopHook) -> Result<Vec<SessionEvent>> {
        let id = hook.session_id.clone();
        self.request(&id, move |reply| SessionInput::Stop { hook, reply })
            .await
    }

    /// Handle a `MessageDisplay` hook: buffer one chunk of the in-flight turn's
    /// assistant message and return the `AssistantStreaming` event to broadcast.
    pub async fn on_message_display(&self, hook: MessageDisplayHook) -> Result<Vec<SessionEvent>> {
        let id = hook.session_id.clone();
        self.request(&id, move |reply| SessionInput::MessageDisplay {
            hook,
            reply,
        })
        .await
    }

    /// Handle a `SessionStart` hook (launch/resume readiness signal).
    pub async fn on_session_start(&self, hook: SessionStartHook) -> Result<Vec<SessionEvent>> {
        let id = hook.session_id.clone();
        self.request(&id, move |reply| SessionInput::SessionStart { hook, reply })
            .await
    }

    /// Handle a `SessionEnd` hook (early launch-failure signal / normal end).
    pub async fn on_session_end(&self, hook: SessionEndHook) -> Result<Vec<SessionEvent>> {
        let id = hook.session_id.clone();
        self.request(&id, move |reply| SessionInput::SessionEnd { hook, reply })
            .await
    }

    /// Handle a `PreToolUse` hook: record the permission request (with its
    /// `tool_use_id`) so the later `tool_result` can resolve it. Routed
    /// through the session's mailbox so the write is ordered with ingestion.
    pub async fn on_pre_tool_use(
        &self,
        session_id: &SessionId,
        tool_name: &str,
        tool_input_json: &str,
        tool_use_id: &str,
        transcript_path: &str,
    ) -> Result<Vec<SessionEvent>> {
        self.request(session_id, |reply| SessionInput::PreToolUse {
            tool_name: tool_name.to_owned(),
            tool_input_json: tool_input_json.to_owned(),
            tool_use_id: tool_use_id.to_owned(),
            transcript_path: transcript_path.to_owned(),
            reply,
        })
        .await
    }

    /// Handle a `PostToolUse` hook: a tool call completed. Delta acts on the
    /// subagent (`Agent`/`Task`) case in two ways. For a foreground subagent
    /// the running window is closed by `tool_use_id`. For a background subagent
    /// the call returned, not the subagent — so the matching launch row is
    /// upgraded with the `agentId` the tool's `tool_result` carries, giving
    /// the eventual `<task-notification>` a fallback correlation key in case
    /// Claude Code strips `<tool-use-id>` from the notification body. Routed
    /// through the session's mailbox so the clear/upgrade is ordered after the
    /// `PreToolUse` that opened the running window.
    pub async fn on_post_tool_use(
        &self,
        session_id: &SessionId,
        tool_name: &str,
        tool_use_id: &str,
        tool_response_json: &str,
        transcript_path: &str,
    ) -> Result<Vec<SessionEvent>> {
        self.request(session_id, |reply| SessionInput::PostToolUse {
            tool_name: tool_name.to_owned(),
            tool_use_id: tool_use_id.to_owned(),
            tool_response_json: tool_response_json.to_owned(),
            transcript_path: transcript_path.to_owned(),
            reply,
        })
        .await
    }

    /// Handle a `PermissionRequest` hook: record the request row, register a
    /// decision waiter on the session's actor, and hand the transport the
    /// receiver it blocks on (with the `PermissionRequested` event to
    /// broadcast *before* blocking).
    ///
    /// The request-id → session index recorded here is what lets
    /// [`Self::decide_permission`] (which only knows the request id) route the
    /// browser's decision to the owning actor.
    pub async fn on_permission_request(
        &self,
        session_id: &SessionId,
        tool_name: &str,
        tool_input_json: &str,
        transcript_path: &str,
    ) -> Result<PermissionWait> {
        let wait = self
            .request(session_id, |reply| SessionInput::PermissionRequest {
                tool_name: tool_name.to_owned(),
                tool_input_json: tool_input_json.to_owned(),
                transcript_path: transcript_path.to_owned(),
                reply,
            })
            .await?;
        self.permission_index
            .lock()
            .expect("permission index poisoned")
            .insert(wait.request_id, session_id.clone());
        Ok(wait)
    }
}
