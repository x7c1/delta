//! [`PendingPermission`]: the permission dialogs awaiting a human answer (an
//! ordered queue, not a single slot), and the oneshot waiters a browser
//! decision resolves.

use tokio::sync::oneshot;

use crate::agent::AgentFileChangeDetail;
use crate::interactor::PermissionDecision;

use super::SessionRuntime;

/// A permission dialog currently awaiting a human answer — in the browser
/// (the notice's Allow/Deny) or in the TUI prompt after the browser-decision
/// wait timed out.
///
/// This is the queryable counterpart of the `PermissionRequested` broadcast:
/// the event is lost for a client whose socket was down when it fired, so the
/// sends envelope (`GET /api/sessions/{id}/sends`) reports the queue's head plus
/// its depth and a reconnecting client rebuilds its notice from a plain refetch,
/// exactly like the turn state. An entry leaves when its request resolves (a
/// browser decision or the correlated `tool_result`), and the whole queue is
/// dropped whenever the turn returns to idle — a dialog cannot outlive its turn.
///
/// Several of these can be outstanding at once: an adapter-backed provider
/// (Codex) runs tool calls in parallel, so one turn can raise N approvals in the
/// same instant. See [`SessionRuntime::enqueue_pending_permission`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingPermission {
    /// The `permission_request` row id (the decision endpoint's key).
    pub request_id: i64,
    pub tool_name: String,
    /// The tool input, serialized as JSON text.
    pub tool_input_json: String,
    /// What allowing this request would do to files on disk, when the provider
    /// stated it. Mirrored here — not only broadcast — because the envelope is
    /// what re-seeds the card after a reconnect: a client that missed the event
    /// must get the same detail from the refetch, or the card it rebuilds would
    /// silently degrade to the input summary.
    pub file_change: Option<AgentFileChangeDetail>,
    /// A directory the request also asks to be allowed to write under for the
    /// rest of the session, when the provider asked for one. Mirrored for the
    /// same reason as [`Self::file_change`], and all the more so: it is the
    /// broadest thing the dialog grants, so a re-seeded card that dropped it
    /// would understate what the user is about to allow.
    pub grant_root: Option<String>,
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

    /// Append a permission dialog to the queue of dialogs awaiting an answer.
    ///
    /// Never displaces a dialog already queued: a provider that runs tool calls
    /// in parallel (Codex) can raise N approvals within one turn, and dropping
    /// all but the last is what deadlocked the turn in the field — the user
    /// answered the last request and the other N-1 waited forever. The queue
    /// keeps every request in arrival order, so the browser can show the head,
    /// report the depth, and walk the rest front to back. Claude's pane-backed
    /// hook blocks serially, so its queue never holds more than one entry.
    ///
    /// Idempotent by request id: a retried hook or a duplicate provider frame
    /// for the same row keeps the single entry in its original position rather
    /// than queueing the same dialog twice.
    pub fn enqueue_pending_permission(&mut self, pending: PendingPermission) {
        if let Some(existing) = self
            .pending_permissions
            .iter_mut()
            .find(|p| p.request_id == pending.request_id)
        {
            *existing = pending;
            return;
        }
        self.pending_permissions.push(pending);
    }

    /// Drop every queued dialog at once, returning the request ids that were
    /// pending (oldest first).
    ///
    /// The wholesale counterpart of [`Self::resolve_pending_permission`], for
    /// the one situation where the queue as a whole becomes moot: the agent
    /// session died, so no dialog in it can ever be answered. Removing them one
    /// by one through the keyed path would promote (and re-broadcast) each
    /// successive head — raising dialogs the same settle is in the middle of
    /// clearing — so the mirror is emptied in one step and the caller broadcasts
    /// one resolution per id.
    pub fn clear_pending_permissions(&mut self) -> Vec<i64> {
        self.pending_permissions
            .drain(..)
            .map(|pending| pending.request_id)
            .collect()
    }

    /// Drop `request_id`'s dialog from the queue, returning the entry that took
    /// over as head when the removal promoted a new one.
    ///
    /// Keyed, so a resolution for a request that is *not* the head removes only
    /// that entry and leaves the visible dialog alone — and a resolution for an
    /// unknown id changes nothing (the same guard the browser notice applies to
    /// `permission_resolved`). The returned head is what keeps the browser from
    /// being left dialog-less while requests are still pending: the caller
    /// re-broadcasts it as a `PermissionRequested`, so a client that only follows
    /// events sees the next dialog without refetching.
    pub fn resolve_pending_permission(&mut self, request_id: i64) -> Option<PendingPermission> {
        let index = self
            .pending_permissions
            .iter()
            .position(|p| p.request_id == request_id)?;
        self.pending_permissions.remove(index);
        // Only removing the head can promote a new one; a resolution from the
        // middle of the queue leaves the visible dialog exactly where it was.
        if index == 0 {
            return self.pending_permissions.first().cloned();
        }
        None
    }
}
