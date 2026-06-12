//! Resolving a pending permission request from the browser.
//!
//! The `PermissionRequest` hook handler registers a oneshot waiter (see
//! `on_permission_request`) and blocks Claude Code until either a decision
//! arrives through here (`POST /api/permissions/{id}/decision`) or the
//! transport's deadline passes — in which case the waiter is abandoned and the
//! hook responds with an empty passthrough, falling back to the interactive
//! TUI prompt exactly as before this endpoint existed.

use crate::error::{Error, Result};
use crate::ports::{SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace};
use crate::Interactor;

/// The browser's answer to a pending permission request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionDecision {
    Allow,
    Deny,
}

impl<T, X, S, W> Interactor<T, X, S, W>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
{
    /// Resolve a pending permission request with the browser's decision.
    ///
    /// Claims the registered waiter first (an atomic take, so two racing
    /// decisions cannot both win), then records the decision on the request
    /// row and wakes the blocked hook handler, which answers Claude Code with
    /// the corresponding `hookSpecificOutput.decision`.
    ///
    /// Returns [`Error::PermissionNotPending`] when no waiter is registered:
    /// the request is unknown, was already decided, or its hook wait timed
    /// out and fell back to the TUI prompt — in every case a UI decision can
    /// no longer take effect, and the caller surfaces that as a conflict.
    pub async fn decide_permission(
        &self,
        request_id: i64,
        decision: PermissionDecision,
    ) -> Result<Vec<SessionEvent>> {
        let sender = self
            .pending_permissions
            .lock()
            .await
            .remove(&request_id)
            .ok_or(Error::PermissionNotPending(request_id))?;

        let allowed = decision == PermissionDecision::Allow;
        let Some(request) = self.store.decide_permission_request(request_id, allowed).await? else {
            // The waiter existed but the row is not `pending` — it was already
            // resolved out from under us (e.g. a tool_result ingested in the
            // same instant). The hook handler still gets the answer; a decided
            // row is left untouched.
            tracing::warn!(
                request_id,
                "permission decision arrived for a row that is no longer pending; \
                 forwarding the decision to the blocked hook without re-deciding the row"
            );
            let _ = sender.send(decision);
            return Ok(Vec::new());
        };

        // A dropped receiver means the hook handler gave up (its timeout fired
        // between our registry take and now). The row is decided either way;
        // Claude Code falls back to the TUI prompt for the actual gating.
        if sender.send(decision).is_err() {
            tracing::warn!(
                request_id,
                "permission decision recorded but the hook wait had already timed out; \
                 the TUI prompt owns the actual gating"
            );
        }

        Ok(vec![SessionEvent::PermissionResolved {
            session_id: request.session_id,
            request_id,
        }])
    }

    /// Abandon the waiter for a permission request whose hook wait timed out.
    ///
    /// The row stays `pending`: the hook responds with an empty passthrough,
    /// Claude Code shows its interactive TUI prompt, and the eventual
    /// `tool_result` resolves the row (see `sync_transcript`).
    pub async fn abandon_permission_decision(&self, request_id: i64) {
        self.pending_permissions.lock().await.remove(&request_id);
    }
}
