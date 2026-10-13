//! Permission requests: recording, deciding, and settling them.

use async_trait::async_trait;

use delta_model::{PermissionRequest, SessionId};

use super::SessionStore;
use crate::error::Result;

/// The tool-permission requests: recording them and settling the pending
/// ones.
///
/// One of the concerns [`SessionStore`] gathers.
#[async_trait]
pub trait PermissionStore: Send + Sync {
    /// Record a tool-permission request and return the created row.
    ///
    /// `tool_use_id` is `Some` for the row `PreToolUse` records for every tool
    /// call (the id later correlates the matching `tool_result`), and `None`
    /// for the row the `PermissionRequest` hook owns for a genuine interactive
    /// dialog (that hook's payload carries no tool-call id).
    async fn record_permission_request(
        &self,
        session_id: &SessionId,
        tool_name: &str,
        tool_input_json: &str,
        tool_use_id: Option<&str>,
    ) -> Result<PermissionRequest>;

    /// Decide a still-`pending` permission request from the browser, recording
    /// `allowed`/`denied` plus `decided_at`. Returns the decided row, or
    /// `None` when the row is unknown or no longer `pending` (an
    /// already-decided row is never flipped).
    async fn decide_permission_request(
        &self,
        request_id: i64,
        allowed: bool,
    ) -> Result<Option<PermissionRequest>>;

    /// Resolve the open (`pending`) permission requests settled by a
    /// just-ingested `tool_result`: the `PreToolUse`-recorded row whose
    /// `tool_use_id` matches, plus any pending dialog row for the same session
    /// (`tool_use_id IS NULL` — the dialog blocks the session, so the next
    /// tool_result to arrive is the one it gated).
    ///
    /// `allowed` records the disposition inferred from the tool_result's error
    /// flag (`false` → the tool was denied). Returns the ids of the rows that
    /// transitioned (empty when there was nothing to resolve).
    async fn resolve_permission_by_tool_use_id(
        &self,
        session_id: &SessionId,
        tool_use_id: &str,
        allowed: bool,
    ) -> Result<Vec<i64>>;

    /// Deny every still-`pending` permission request of a session, recording
    /// `reason` on each row alongside the `denied` status and `decided_at`.
    ///
    /// The disposition for requests whose answer can no longer be delivered —
    /// the agent process ended while approvals were outstanding. `denied` is the
    /// honest record: the tool was never allowed to run, and the provider that
    /// asked is gone, so nothing can act on the row afterwards. Leaving them
    /// `pending` is the failure this exists to prevent: the audit trail would
    /// claim someone is still being asked, and the row would never settle.
    /// `reason` is what keeps the trail readable — the status alone cannot
    /// distinguish a user's Deny from a request nobody could answer.
    ///
    /// Returns the ids of the rows that transitioned (empty when nothing was
    /// pending), so the caller can settle their client-visible notices.
    async fn deny_pending_permission_requests(
        &self,
        session_id: &SessionId,
        reason: &str,
    ) -> Result<Vec<i64>>;
}

#[async_trait]
impl PermissionStore for Box<dyn SessionStore> {
    async fn record_permission_request(
        &self,
        session_id: &SessionId,
        tool_name: &str,
        tool_input_json: &str,
        tool_use_id: Option<&str>,
    ) -> Result<PermissionRequest> {
        (**self)
            .record_permission_request(session_id, tool_name, tool_input_json, tool_use_id)
            .await
    }

    async fn decide_permission_request(
        &self,
        request_id: i64,
        allowed: bool,
    ) -> Result<Option<PermissionRequest>> {
        (**self)
            .decide_permission_request(request_id, allowed)
            .await
    }

    async fn resolve_permission_by_tool_use_id(
        &self,
        session_id: &SessionId,
        tool_use_id: &str,
        allowed: bool,
    ) -> Result<Vec<i64>> {
        (**self)
            .resolve_permission_by_tool_use_id(session_id, tool_use_id, allowed)
            .await
    }

    async fn deny_pending_permission_requests(
        &self,
        session_id: &SessionId,
        reason: &str,
    ) -> Result<Vec<i64>> {
        (**self)
            .deny_pending_permission_requests(session_id, reason)
            .await
    }
}
