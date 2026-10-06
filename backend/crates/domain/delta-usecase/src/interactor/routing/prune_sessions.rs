//! Removing old closed sessions in bulk, one routed removal per session.

use std::time::SystemTime;

use delta_model::SessionId;

use crate::error::{Error, Result};
use crate::interactor::Interactor;
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};
use crate::session_prune::{
    PruneCriteria, PruneReport, SessionKeptItem, SkipReason, SkippedSession,
};
use crate::session_removal::DiskItem;

impl<T, X, S, W, G> Interactor<T, X, S, W, G>
where
    T: TmuxDriver + 'static,
    X: Transcript + 'static,
    S: SessionStore + 'static,
    W: Workspace + 'static,
    G: GitWorktree + 'static,
{
    /// The sessions [`Self::prune_sessions`] would remove at `now`, oldest
    /// first, without removing anything.
    ///
    /// The stored rows that match `criteria`, less the sessions that are live
    /// right now (open, or still starting): those are never removed, so they
    /// are not counted as candidates either.
    pub async fn prune_candidates(
        &self,
        criteria: &PruneCriteria,
        now: SystemTime,
    ) -> Result<Vec<SessionId>> {
        let matching = self.matching_sessions(criteria, now).await?;
        let live = self.live_session_ids().await;
        Ok(matching
            .into_iter()
            .filter(|id| !live.contains(id))
            .collect())
    }

    /// Remove every closed session matching `criteria` at `now`, each exactly
    /// as [`Self::delete_session`] removes one.
    ///
    /// Going through the single removal is the point: every refusal it makes
    /// and the rule on the worktree and branch Delta created for a session
    /// apply unchanged, so a worktree with uncommitted work or an unmerged
    /// branch is kept and reported here as it would be for one session.
    ///
    /// A matching session is skipped, not failed on: one that is open or
    /// still starting (the single removal's two refusals), one already gone,
    /// and one whose removal failed outright (logged at `warn`) are reported
    /// in [`PruneReport::skipped`] and the rest are still removed. Only the
    /// query for the matching rows can fail the whole call.
    ///
    /// The caller broadcasts `SessionRemoved` for each removed session.
    pub async fn prune_sessions(
        &self,
        criteria: &PruneCriteria,
        now: SystemTime,
    ) -> Result<PruneReport> {
        let matching = self.matching_sessions(criteria, now).await?;
        Ok(self.remove_sessions(matching, &mut Vec::new()).await)
    }

    /// Remove each of `session_ids` through the single removal
    /// ([`Self::delete_session`]), skipping — never failing on — one that is
    /// open, still starting, already gone, or whose removal failed (logged at
    /// `warn`). The worktrees, branches and trust entries the removals took
    /// with them are appended to `removed_items`, in order.
    ///
    /// The loop behind both [`Self::prune_sessions`] and
    /// [`Self::erase_everything`].
    pub(super) async fn remove_sessions(
        &self,
        session_ids: Vec<SessionId>,
        removed_items: &mut Vec<DiskItem>,
    ) -> PruneReport {
        let mut report = PruneReport::default();
        for session_id in session_ids {
            match self.delete_session(&session_id).await {
                Ok(removal) => {
                    tracing::info!(%session_id, "removed session; {removal}");
                    removed_items.extend(removal.removed);
                    report
                        .kept
                        .extend(removal.kept.into_iter().map(|kept| SessionKeptItem {
                            session_id: session_id.clone(),
                            kept,
                        }));
                    report.removed.push(session_id);
                }
                Err(err) => {
                    let reason = skip_reason(err);
                    if let SkipReason::Failed(error) = &reason {
                        tracing::warn!(%session_id, error, "removing a session failed; skipping it");
                    }
                    report.skipped.push(SkippedSession { session_id, reason });
                }
            }
        }
        report
    }

    /// The stored rows matching `criteria` at `now`, oldest first.
    async fn matching_sessions(
        &self,
        criteria: &PruneCriteria,
        now: SystemTime,
    ) -> Result<Vec<SessionId>> {
        self.store
            .list_prunable_sessions(&criteria.cutoff(now), &criteria.row_statuses())
            .await
    }
}

/// Why a single removal refused or failed, as a reason to skip the session.
fn skip_reason(err: Error) -> SkipReason {
    match err {
        Error::SessionOpen(_) => SkipReason::Open,
        Error::SessionSpawning(_) => SkipReason::Starting,
        Error::SessionNotFound(_) => SkipReason::Gone,
        other => SkipReason::Failed(other.to_string()),
    }
}
