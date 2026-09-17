//! Permission decisions: resolving a pending permission request with the
//! browser's answer, or abandoning its waiter when the hook wait timed out.
//! Both are keyed only by request id, so both route through the request-id →
//! session index the `PermissionRequest` hook recorded.

use crate::error::{Error, Result};
use crate::interactor::session_actor::input::SessionInput;
use crate::interactor::{Interactor, PermissionDecision};
use crate::ports::{GitWorktree, SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace};

impl<T, X, S, W, G> Interactor<T, X, S, W, G>
where
    T: TmuxDriver + 'static,
    X: Transcript + 'static,
    S: SessionStore + 'static,
    W: Workspace + 'static,
    G: GitWorktree + 'static,
{
    /// Resolve a pending permission request with the browser's decision.
    ///
    /// Claims the request's index entry first (an atomic take, so two racing
    /// decisions cannot both win), then routes the decision to the owning
    /// session's actor, which wakes the blocked hook handler.
    ///
    /// Returns [`Error::PermissionNotPending`] when no waiter can be reached:
    /// the request is unknown, was already decided, its hook wait timed out and
    /// fell back to the TUI prompt, or its agent session ended — no decision can
    /// reach a wire that is gone, and the two ends reject it from different
    /// places: a death's settle drops the index entry here, while a close leaves
    /// the entry in place and the actor rejects the decision (its open agent, and
    /// with it the wire to answer on, is gone). In every case a UI decision can
    /// no longer take effect, and the caller surfaces that as a conflict.
    ///
    /// Returns [`Error::PermissionDecisionUnsupported`] when the decision itself
    /// is one this session's provider cannot express (a session-scoped allow
    /// against a provider that does not declare it). That verdict is reached
    /// before anything is claimed or written, so the claim taken above is handed
    /// **back**: unlike every other failure here the request is still pending and
    /// still answerable, and a client that retries with a plain allow must not
    /// meet a spurious `409` left behind by the rejected attempt.
    pub async fn decide_permission(
        &self,
        request_id: i64,
        decision: PermissionDecision,
    ) -> Result<Vec<SessionEvent>> {
        let session_id = self
            .permission_index
            .lock()
            .expect("permission index poisoned")
            .remove(&request_id)
            .ok_or(Error::PermissionNotPending(request_id))?;
        let outcome = self
            .request(&session_id, |reply| SessionInput::DecidePermission {
                request_id,
                decision,
                reply,
            })
            .await;
        if matches!(outcome, Err(Error::PermissionDecisionUnsupported(_))) {
            self.permission_index
                .lock()
                .expect("permission index poisoned")
                .insert(request_id, session_id);
        }
        outcome
    }

    /// Abandon the waiter for a permission request whose hook wait timed out.
    ///
    /// The row stays `pending`: the hook responds with an empty passthrough,
    /// Claude Code shows its interactive TUI prompt, and the eventual
    /// `tool_result` resolves the row (see `sync_transcript`).
    pub async fn abandon_permission_decision(&self, request_id: i64) {
        let session_id = self
            .permission_index
            .lock()
            .expect("permission index poisoned")
            .remove(&request_id);
        if let Some(id) = session_id {
            self.sessions
                .post(&id, SessionInput::AbandonPermission { request_id });
        }
    }
}
