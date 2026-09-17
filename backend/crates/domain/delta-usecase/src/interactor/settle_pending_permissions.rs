//! Settling the permission dialogs a session strands when its agent stops
//! driving it, shared by every path that reaches that point.
//!
//! Two do: an adapter-backed session whose backing process died
//! ([`settle_agent_session_end`]) and the teardown a bound session goes through
//! when it is closed ([`tear_down_bound_session`]) — whether a person pressed
//! Close or the liveness sweep found the pane gone. They differ in exactly one
//! thing, the sentence recorded on the rows, so that is the only thing this
//! routine parameterises.
//!
//! Sharing it is not mere tidiness: a request that can no longer be answered
//! has to be settled in *four* places at once — the row, the queryable mirror,
//! the decision-routing index and the blocked hook — and a copy that forgot one
//! of them would strand exactly the state the settle exists to clear.
//!
//! [`settle_agent_session_end`]: super::agent_event
//! [`tear_down_bound_session`]: super::lifecycle

use std::collections::BTreeSet;

use crate::agent::AgentEvent;
use crate::interactor::agent_permission::reduce_permission_event;
use crate::interactor::session_actor::actor::SessionContext;
use crate::interactor::PermissionDecision;
use crate::ports::{GitWorktree, SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace};

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Settle every permission request of this session that can no longer be
    /// answered: deny its row with `reason`, empty the queryable mirror, drop
    /// the decision-routing entry, release a hook still blocked on it, and
    /// return one [`SessionEvent::PermissionResolved`] per request for the
    /// caller to announce.
    ///
    /// `reason` is the `decision_reason` the trail carries (see
    /// [`SessionStore::deny_pending_permission_requests`]): `denied` alone would
    /// be indistinguishable from a user's Deny, so each caller passes a sentence
    /// naming why nobody could answer.
    ///
    /// The events are **returned** rather than emitted, because the two callers
    /// reach the browser by different seams — the teardown hands its events to
    /// the caller that broadcasts them, the death path emits on the async seam —
    /// and neither may have this settle reorder the events it produces around
    /// them.
    ///
    /// What each step is for:
    ///
    /// - **The store sweep and the runtime queue are unioned** rather than
    ///   trusting either alone: the sweep is the authority on rows (it also
    ///   catches a row the mirror never held), while the queue is what a browser
    ///   is actually showing (it also catches a dialog whose row was settled by
    ///   some other path). Every id from either side gets its settle broadcast,
    ///   so no notice is left on screen and no row is left `pending`.
    /// - **The queue is emptied in one step, before the per-request settles.**
    ///   Resolving it entry by entry would promote each successive head and
    ///   re-broadcast it as a fresh dialog — raising dialogs this very settle is
    ///   in the middle of clearing (see
    ///   [`SessionRuntime::clear_pending_permissions`]). With the mirror already
    ///   empty, the reducer below produces exactly the settle and promotes
    ///   nothing.
    /// - **The decision-routing entry is dropped**, so a decision POST that
    ///   races this settle answers `permission_not_pending` (409) rather than
    ///   reaching the actor and failing on a wire that is gone (500).
    /// - **A blocked hook waiter is released with a `Deny`.** A pane-backed
    ///   (Claude) dialog is held open by a `PermissionRequest` hook parked on a
    ///   oneshot; nothing will answer it now, so without this the hook would sit
    ///   there until its own decision deadline expired. `Deny` is the same
    ///   disposition the row was just recorded with — the tool never ran. (An
    ///   adapter-backed request has no waiter, so this is a no-op on that path.)
    ///
    /// A sweep failure is logged and the rest of the settle continues: a stuck
    /// dialog is worse than an unsettled audit row.
    ///
    /// [`SessionStore::deny_pending_permission_requests`]: crate::ports::SessionStore::deny_pending_permission_requests
    /// [`SessionRuntime::clear_pending_permissions`]: crate::interactor::session_actor::runtime::SessionRuntime::clear_pending_permissions
    pub(in crate::interactor) async fn settle_pending_permissions(
        &mut self,
        reason: &str,
    ) -> Vec<SessionEvent> {
        let denied = match self
            .store
            .deny_pending_permission_requests(self.id, reason)
            .await
        {
            Ok(ids) => ids,
            Err(err) => {
                tracing::error!(
                    session_id = %self.id,
                    error = %err,
                    "failed to deny the pending permission requests of a session whose agent \
                     is gone; their rows stay pending, but their dialogs are still cleared below"
                );
                Vec::new()
            }
        };
        let mirrored = self.state.clear_pending_permissions();
        let request_ids: BTreeSet<i64> = denied.into_iter().chain(mirrored).collect();
        let mut events = Vec::with_capacity(request_ids.len());
        for request_id in request_ids {
            self.permission_index
                .lock()
                .expect("permission index poisoned")
                .remove(&request_id);
            if let Some(waiter) = self.state.take_permission_waiter(request_id) {
                // The receiver is gone if the hook already gave up on its own
                // deadline; either way the row is settled, so the send is
                // fire-and-forget.
                let _ = waiter.send(PermissionDecision::Deny);
            }
            // Through the shared reducer, so the broadcast is byte-identical to
            // every other resolution.
            let event = AgentEvent::PermissionResolved {
                request_id: request_id.to_string(),
                // The row was recorded denied: the tool never ran.
                decision: PermissionDecision::Deny,
            };
            events.extend(reduce_permission_event(self.state, self.id, &event));
        }
        events
    }
}
