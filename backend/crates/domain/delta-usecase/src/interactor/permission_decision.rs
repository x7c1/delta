//! Resolving a pending permission request from the browser.
//!
//! The `PermissionRequest` hook handler registers a oneshot waiter (see
//! `on_permission_request`) and blocks Claude Code until either a decision
//! arrives through here (`POST /api/permissions/{id}/decision`) or the
//! transport's deadline passes — in which case the waiter is abandoned and the
//! hook responds with an empty passthrough, falling back to the interactive
//! TUI prompt exactly as before this endpoint existed.
//!
//! Both paths execute inside the session's actor: the routing layer resolves
//! the request id to its owning session (the interactor's permission index)
//! and posts here, so a decision, an abandonment, and the hook registration
//! can never interleave for one session.

use crate::agent::AgentEvent;
use crate::error::{Error, Result};
use crate::interactor::agent_permission::reduce_permission_event;
use crate::interactor::session_actor::actor::SessionContext;
use crate::ports::{GitWorktree, SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace};

/// The browser's answer to a pending permission request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionDecision {
    Allow,
    Deny,
}

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Resolve a pending permission request with the browser's decision.
    ///
    /// Claims the registered waiter first (the mailbox serializes racing
    /// decisions, so only the first finds it), then records the decision on
    /// the request row and wakes the blocked hook handler, which answers
    /// Claude Code with the corresponding `hookSpecificOutput.decision`.
    ///
    /// Returns [`Error::PermissionNotPending`] when no waiter is registered:
    /// the request was already decided, or its hook wait timed out and fell
    /// back to the TUI prompt — in either case a UI decision can no longer
    /// take effect, and the caller surfaces that as a conflict.
    pub(in crate::interactor) async fn decide_permission(
        &mut self,
        request_id: i64,
        decision: PermissionDecision,
    ) -> Result<Vec<SessionEvent>> {
        // An adapter-backed (Codex) permission carries no hook waiter: its
        // decision is answered over the provider's wire, not by waking a blocked
        // hook. The presence of a row → provider-token correlation is what marks
        // it, so branch here before the Claude waiter path.
        if let Some(token) = self
            .state
            .agent_permission_token(request_id)
            .map(str::to_owned)
        {
            return self
                .decide_agent_permission(request_id, &token, decision)
                .await;
        }

        let sender = self
            .state
            .take_permission_waiter(request_id)
            .ok_or(Error::PermissionNotPending(request_id))?;

        let allowed = decision == PermissionDecision::Allow;
        let Some(request) = self
            .store
            .decide_permission_request(request_id, allowed)
            .await?
        else {
            // The waiter existed but the row is not `pending` — it was already
            // resolved out from under us (e.g. a tool_result ingested in the
            // same instant). The hook handler still gets the answer; a decided
            // row is left untouched, and its resolution already cleared the
            // mirror, so this path emits no further broadcast. Clear the mirror
            // defensively (keyed, so it cannot wipe a newer dialog).
            self.state.resolve_pending_permission(request_id);
            tracing::warn!(
                request_id,
                "permission decision arrived for a row that is no longer pending; \
                 forwarding the decision to the blocked hook without re-deciding the row"
            );
            let _ = sender.send(decision);
            return Ok(Vec::new());
        };

        // A dropped receiver means the hook handler gave up (its timeout fired
        // between the waiter take and now). The row is decided either way;
        // Claude Code falls back to the TUI prompt for the actual gating.
        if sender.send(decision).is_err() {
            tracing::warn!(
                request_id,
                "permission decision recorded but the hook wait had already timed out; \
                 the TUI prompt owns the actual gating"
            );
        }

        // Route the resolution through the permission reducer: it drops the
        // queryable dialog mirror (keyed, so a stale id cannot wipe a newer
        // dialog) and produces the `PermissionResolved` broadcast that settles
        // the browser notice.
        let event = AgentEvent::PermissionResolved {
            request_id: request_id.to_string(),
            decision,
        };
        Ok(reduce_permission_event(
            self.state,
            &request.session_id,
            &event,
        ))
    }

    /// Answer an adapter-backed (Codex) permission decision.
    ///
    /// Records the disposition on the request row (the audit trail the sends
    /// envelope reports), then hands the decision to the adapter over the trait —
    /// translating the Delta row id back to the adapter-scoped provider `token`
    /// it was correlated with. The adapter answers the provider's wire and emits
    /// an [`AgentEvent::PermissionResolved`] on the session's stream; the event
    /// pump ingests that and drives the mirror-clear + settle broadcast (and
    /// drops the correlation). So this returns no synchronous events — the
    /// browser notice settles through the same async seam every other Codex
    /// signal takes.
    async fn decide_agent_permission(
        &mut self,
        request_id: i64,
        token: &str,
        decision: PermissionDecision,
    ) -> Result<Vec<SessionEvent>> {
        let agent = self
            .state
            .open_agent()
            .cloned()
            .ok_or(Error::PermissionNotPending(request_id))?;

        // Record the row disposition. A row that is no longer `pending` (resolved
        // out from under us) is left untouched — the decision still reaches the
        // provider below, which is what actually gates the tool.
        let allowed = decision == PermissionDecision::Allow;
        self.store
            .decide_permission_request(request_id, allowed)
            .await?;

        agent
            .adapter
            .resolve_permission(&agent.handle, token, decision)
            .await?;
        Ok(Vec::new())
    }

    /// Abandon the waiter for a permission request whose hook wait timed out.
    ///
    /// The row stays `pending`: the hook responds with an empty passthrough,
    /// Claude Code shows its interactive TUI prompt, and the eventual
    /// `tool_result` resolves the row (see `sync_transcript`).
    pub(in crate::interactor) fn abandon_permission_decision(&mut self, request_id: i64) {
        self.state.take_permission_waiter(request_id);
    }
}
