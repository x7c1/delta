//! [`AgentAdapter`] for [`CodexAppServerAdapter`].

use async_trait::async_trait;
use serde_json::json;
use tokio::sync::mpsc;

use delta_usecase::{
    AgentAdapter, AgentCapabilities, AgentContentSource, AgentEvent, AgentEventStream,
    AgentProvider, AgentSessionHandle, ContentSourceRequest, Error as UsecaseError, LaunchRequest,
    PermissionDecision, PtyHandle, Result as UsecaseResult, ResumeRequest, SendReceipt,
    SendRequest, SessionEndReason,
};

use super::{
    inject_message_item, thread_resume_params, thread_start_params, to_usecase_err,
    CodexAppServerAdapter, CODEX_CAPABILITIES, DECISION_ACCEPT, DECISION_ACCEPT_FOR_SESSION,
    DECISION_DECLINE,
};
use crate::codex_content_source;

#[async_trait]
impl AgentAdapter for CodexAppServerAdapter {
    fn provider(&self) -> AgentProvider {
        AgentProvider::Codex
    }

    fn capabilities(&self) -> AgentCapabilities {
        CODEX_CAPABILITIES
    }

    async fn launch(&self, req: LaunchRequest) -> UsecaseResult<AgentSessionHandle> {
        // The server assigns the thread id; Delta's own session id is not pinned
        // onto it (that is the `ProviderReturnsId` identity model). The workdir
        // rides `thread/start` as `cwd`, the user's selected launch options ride
        // it as further fields, and a worktree session's source repository
        // contributes the sandbox grant for its real git directory (see
        // [`thread_start_params`]).
        let params = thread_start_params(
            &req.workdir,
            &req.launch_options,
            req.worktree_repo_root.as_deref(),
        )?;
        // `thread/start` is scoped to Delta's session id — the only id that
        // exists at this point (the thread id is what this call returns), and the
        // one the inspector is keyed by, so a session's log opens with its own
        // launch rather than starting mid-conversation.
        let started = self
            .conn
            .start_thread(Some(&req.session_id), Some(params))
            .await
            .map_err(to_usecase_err)?;
        let handle = self.register_session(req.session_id, started);
        // A first prompt from the composer's opening Send starts a turn straight
        // away, mirroring how Claude's spawn path auto-submits its launch prompt.
        if let Some(prompt) = req.first_prompt {
            self.start_turn(&handle, prompt).await?;
        }
        Ok(handle)
    }

    /// Reattach to an existing thread with `thread/resume`.
    ///
    /// No launch options ride this call. `ThreadResumeParams`' config fields
    /// (`model`, `sandbox`, `approvalPolicy`, `config`, …) are documented as
    /// *overrides* for the resumed thread, so omitting them keeps whatever
    /// `thread/start` configured — and the core has no per-session record of
    /// which options were selected to replay anyway (see
    /// `resume_adapter_agent` in the core for the full reasoning).
    ///
    /// The worktree git-directory grant is the one exception, and for the
    /// opposite reason: it is not a selected option Delta would be replaying but a
    /// fact about the session's own working directory, which the core re-derives
    /// from the session row on every resume. Sending it keeps a resumed worktree
    /// session sandboxed exactly like a fresh one; omitting it would bring the
    /// approval prompts back after a restart.
    async fn resume(&self, req: ResumeRequest) -> UsecaseResult<AgentSessionHandle> {
        let params = thread_resume_params(
            &req.provider_session_id,
            &req.workdir,
            req.worktree_repo_root.as_deref(),
        );
        let started = self
            .conn
            .resume_thread(Some(&req.session_id), Some(params))
            .await
            .map_err(to_usecase_err)?;
        Ok(self.register_session(req.session_id, started))
    }

    async fn send(
        &self,
        handle: &AgentSessionHandle,
        req: SendRequest,
    ) -> UsecaseResult<SendReceipt> {
        self.start_turn(handle, req.text).await
    }

    /// Inject hidden per-turn context via `thread/inject_items`: append one
    /// Responses API user-message item to the thread's model-visible history, so
    /// the next `turn/start` runs with the branched-from passage in context
    /// without it ever appearing in the visible prompt.
    ///
    /// `ThreadInjectItemsParams` (vendored schema) is `{ threadId, items }`,
    /// where `items` are raw Responses API items; a user message is a
    /// `MessageResponseItem` — `{ type: "message", role: "user", content: [{
    /// type: "input_text", text }] }` (see `ResponseItem` / `ContentItem` in the
    /// vendored v2 schema). The response (`ThreadInjectItemsResponse`) is an
    /// empty object, so nothing is read off it.
    async fn inject_context(&self, handle: &AgentSessionHandle, text: &str) -> UsecaseResult<()> {
        let params = json!({
            "threadId": handle.provider_session_id,
            "items": [inject_message_item(text)],
        });
        self.conn
            .request(
                self.scope_of(&handle.key).as_deref(),
                "thread/inject_items",
                Some(params),
            )
            .await
            .map_err(to_usecase_err)?;
        Ok(())
    }

    /// Interrupt the turn currently in flight on this thread.
    ///
    /// Real `turn/interrupt` params are `{threadId, turnId}` (both required), so
    /// the adapter sends the turn id it is tracking for this session. When no
    /// turn is in flight (nothing tracked) there is nothing to interrupt, so this
    /// is a no-op success rather than an RPC the server would reject for a missing
    /// turn id. The turn is ended by the resulting `turn/completed{interrupted}`
    /// flowing back through the same translation path as any other completion.
    async fn interrupt(&self, handle: &AgentSessionHandle) -> UsecaseResult<()> {
        let turn_id = {
            let sessions = self.sessions.lock().expect("sessions mutex poisoned");
            sessions.get(&handle.key).and_then(|session| {
                session
                    .shared
                    .current_turn_id
                    .lock()
                    .expect("current turn id mutex poisoned")
                    .clone()
            })
        };
        let Some(turn_id) = turn_id else {
            // No turn in flight: nothing to interrupt.
            return Ok(());
        };
        self.conn
            .request(
                self.scope_of(&handle.key).as_deref(),
                "turn/interrupt",
                Some(json!({
                    "threadId": handle.provider_session_id,
                    "turnId": turn_id,
                })),
            )
            .await
            .map_err(to_usecase_err)?;
        Ok(())
    }

    /// Answer an open approval request with a decision, mapping it to the Codex
    /// wire value (allow → `accept`, session-scoped allow → `acceptForSession`,
    /// deny → `decline`), and emit
    /// [`AgentEvent::PermissionResolved`].
    ///
    /// This is the entry point — reachable through `Arc<dyn AgentAdapter>` —
    /// through which a browser decision reaches the provider. `request_id` is
    /// the adapter-scoped token surfaced on the matching `PermissionRequested`,
    /// which keys the open approval's verbatim wire id. Errors when the session
    /// or the request id is unknown (already answered, or never open).
    async fn resolve_permission(
        &self,
        handle: &AgentSessionHandle,
        request_id: &str,
        decision: PermissionDecision,
    ) -> UsecaseResult<()> {
        let (wire_id, scope) = {
            let sessions = self.sessions.lock().expect("sessions mutex poisoned");
            let session = sessions
                .get(&handle.key)
                .ok_or_else(|| UsecaseError::Agent(format!("unknown session `{}`", handle.key)))?;
            let removed = session
                .shared
                .approvals
                .lock()
                .expect("approvals mutex poisoned")
                .remove(request_id);
            (removed, session.session_id.clone())
        };
        let wire_id = wire_id.ok_or_else(|| {
            UsecaseError::Agent(format!(
                "permission request `{request_id}` is not awaiting a decision"
            ))
        })?;
        let decision_value = match decision {
            PermissionDecision::Allow => DECISION_ACCEPT,
            PermissionDecision::AllowForSession => DECISION_ACCEPT_FOR_SESSION,
            PermissionDecision::Deny => DECISION_DECLINE,
        };
        // Both approval kinds that reach here — command execution and file change
        // — share the `{ "decision": … }` response shape, so a single reply
        // serves both without needing to remember which method opened the
        // request (the permissions approval, whose response differs, never
        // becomes an open approval — it takes the unsupported path).
        self.conn
            .respond(
                Some(&scope),
                &wire_id,
                json!({ "decision": decision_value }),
            )
            .await
            .map_err(to_usecase_err)?;
        self.emit(
            &handle.key,
            AgentEvent::PermissionResolved {
                request_id: request_id.to_owned(),
                decision,
            },
        );
        Ok(())
    }

    async fn close(&self, handle: &AgentSessionHandle) -> UsecaseResult<()> {
        // v1 models no per-thread close RPC: the shared connection stays up for
        // its other threads. Surface the end and drop the session's plumbing
        // (which aborts its translation task).
        self.emit(
            &handle.key,
            AgentEvent::SessionEnded {
                reason: SessionEndReason::Closed,
            },
        );
        let closed = self
            .sessions
            .lock()
            .expect("sessions mutex poisoned")
            .remove(&handle.key);
        // Release the session's comms-log buffer with the session itself: a
        // closed session has no live wire, so holding its frames would only leak
        // one buffer per closed session for the process's lifetime while showing
        // a log that can never grow again.
        if let Some(closed) = closed {
            self.conn.comms_log().discard(&closed.session_id);
        }
        Ok(())
    }

    fn events(&self, handle: &AgentSessionHandle) -> AgentEventStream {
        let mut sessions = self.sessions.lock().expect("sessions mutex poisoned");
        match sessions.get_mut(&handle.key).and_then(|s| s.rx.take()) {
            Some(rx) => AgentEventStream::new(rx),
            None => {
                // Already handed out, or an unknown/closed session: an
                // already-closed stream rather than a panic.
                let (_tx, rx) = mpsc::unbounded_channel();
                AgentEventStream::new(rx)
            }
        }
    }

    async fn attach_terminal(
        &self,
        _handle: &AgentSessionHandle,
    ) -> UsecaseResult<Option<PtyHandle>> {
        // Codex is `TerminalCapability::NoTerminal`: there is nothing to attach.
        Ok(None)
    }

    fn content_source(
        &self,
        handle: &AgentSessionHandle,
        req: ContentSourceRequest,
    ) -> Box<dyn AgentContentSource> {
        // Codex pushes structured `item/*` / `turn/*` frames, so its event pump
        // folds them into canonical messages through this accumulator (the
        // `CodexConversationSource`), rather than reading a transcript. The
        // launch site rides the neutral request; the model is the one fact only
        // this adapter holds, recorded from the thread's opening response when
        // the session was registered.
        codex_content_source(req, self.session_model(&handle.key))
    }
}
