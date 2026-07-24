//! [`PendingPermission`]: the permission dialog awaiting a human answer, and
//! the oneshot waiters a browser decision resolves.

use tokio::sync::oneshot;

use crate::interactor::PermissionDecision;

use super::SessionRuntime;

/// A permission dialog currently awaiting a human answer — in the browser
/// (the notice's Allow/Deny) or in the TUI prompt after the browser-decision
/// wait timed out.
///
/// This is the queryable counterpart of the `PermissionRequested` broadcast:
/// the event is lost for a client whose socket was down when it fired, so the
/// sends envelope (`GET /api/sessions/{id}/sends`) reports this state and a
/// reconnecting client rebuilds its notice from a plain refetch, exactly like
/// the turn state. Cleared when the request resolves (a browser decision or
/// the correlated `tool_result`) and whenever the turn returns to idle — a
/// dialog cannot outlive its turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingPermission {
    /// The `permission_request` row id (the decision endpoint's key).
    pub request_id: i64,
    pub tool_name: String,
    /// The tool input, serialized as JSON text.
    pub tool_input_json: String,
}

impl SessionRuntime {
    /// Register a oneshot waiter for a permission request the browser may
    /// decide, keyed by request-row id.
    pub fn insert_permission_waiter(
        &mut self,
        request_id: i64,
        sender: oneshot::Sender<PermissionDecision>,
    ) {
        self.permission_waiters.insert(request_id, sender);
    }

    /// Claim the waiter for a permission request, if it is still registered.
    /// Taking it is what makes two racing decisions unambiguous: the mailbox
    /// serializes them, and only the first finds the waiter.
    pub fn take_permission_waiter(
        &mut self,
        request_id: i64,
    ) -> Option<oneshot::Sender<PermissionDecision>> {
        self.permission_waiters.remove(&request_id)
    }

    /// Record the permission dialog now awaiting an answer (a new dialog
    /// replaces a stale one — `claude` shows one at a time).
    pub fn set_pending_permission(&mut self, pending: PendingPermission) {
        self.pending_permission = Some(pending);
    }

    /// Drop the pending dialog if `request_id` is the one it tracks. Keyed so
    /// a stale resolution can never wipe a newer dialog's state — the same
    /// guard the browser notice applies to `permission_resolved`.
    pub fn resolve_pending_permission(&mut self, request_id: i64) {
        if self
            .pending_permission
            .as_ref()
            .is_some_and(|p| p.request_id == request_id)
        {
            self.pending_permission = None;
        }
    }
}
