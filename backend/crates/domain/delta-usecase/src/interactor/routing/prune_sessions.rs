//! Removing old closed sessions in bulk, one routed removal per session.

use std::time::SystemTime;

use delta_model::SessionId;

use crate::error::{Error, Result};
use crate::interactor::Interactor;
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};
use crate::session_prune::{
    PruneCriteria, PruneReport, SessionKeptItem, SkipReason, SkippedSession,
};
use crate::session_removal::{DiskItem, KeepReason, SessionRemoval};

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
    /// branch is kept and reported here as it would be for one session. Two
    /// removed sessions that shared a worktree are settled once both are
    /// gone, so the first one's merged branch goes too (see
    /// `Self::remove_sessions`).
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
    /// Two sessions can share one worktree (a second session started on a
    /// branch that already had one). The first one's removal keeps the
    /// worktree because the second still works in it, and the second's then
    /// removes it, leaving the first one's branch behind. So once every row
    /// is gone, each session whose worktree was kept only because another
    /// session worked in it is revisited under the same rule
    /// ([`InteractorCore::remove_owned_worktree`]): its branch alone when the
    /// batch already removed the worktree, the whole rule otherwise (a
    /// session outside the batch still working in it keeps it again). The
    /// report then says what is left in the end: an item the batch removed
    /// is not also listed as kept, and an item two sessions kept is listed
    /// once, with the reason about the item itself rather than "Delta did
    /// not create it" for the session that merely shared it.
    ///
    /// The loop behind both [`Self::prune_sessions`] and
    /// [`Self::erase_everything`].
    ///
    /// [`InteractorCore::remove_owned_worktree`]: crate::interactor::InteractorCore::remove_owned_worktree
    pub(super) async fn remove_sessions(
        &self,
        session_ids: Vec<SessionId>,
        removed_items: &mut Vec<DiskItem>,
    ) -> PruneReport {
        let mut report = PruneReport::default();
        let mut shared = Vec::new();
        for session_id in session_ids {
            // Read before the removal deletes it: revisiting a shared worktree
            // needs the row's directory, repository and branch.
            let row = match self.store.session(&session_id).await {
                Ok(row) => row,
                Err(err) => {
                    tracing::warn!(
                        %session_id,
                        error = %err,
                        "could not read the session before removing it; \
                         a worktree it shares will not be revisited"
                    );
                    None
                }
            };
            match self.delete_session(&session_id).await {
                Ok(removal) => {
                    tracing::info!(%session_id, "removed session; {removal}");
                    report.removed.push(session_id.clone());
                    match row.filter(|_| kept_for_another_session(&removal)) {
                        Some(row) => shared.push((session_id, row)),
                        None => record(&mut report, removed_items, session_id, removal),
                    }
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
        for (session_id, row) in shared {
            let removal = if removed_items.contains(&DiskItem::Worktree(row.cwd.clone())) {
                self.remove_owned_branch_of_removed_worktree(&session_id, &row)
                    .await
            } else {
                self.remove_owned_worktree(&session_id, &row).await
            };
            tracing::info!(%session_id, "revisited the worktree the session shared; {removal}");
            record(&mut report, removed_items, session_id, removal);
        }
        settle_kept(&mut report.kept, removed_items);
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

/// Whether `removal` kept the session's worktree because another session
/// still worked in it.
fn kept_for_another_session(removal: &SessionRemoval) -> bool {
    removal.kept.iter().any(|kept| {
        matches!(kept.item, DiskItem::Worktree(_))
            && kept.reason == KeepReason::InUseByAnotherSession
    })
}

/// Add what removing `session_id` removed and kept to the batch's lists.
fn record(
    report: &mut PruneReport,
    removed_items: &mut Vec<DiskItem>,
    session_id: SessionId,
    removal: SessionRemoval,
) {
    removed_items.extend(removal.removed);
    report
        .kept
        .extend(removal.kept.into_iter().map(|kept| SessionKeptItem {
            session_id: session_id.clone(),
            kept,
        }));
}

/// Make `kept` say what the batch left in the end: drop an item the batch
/// removed (another session sharing it removed it later), and list an item
/// several sessions kept once — the first entry, unless a later one says why
/// the item itself stays where the first only says Delta did not create it
/// for that session.
fn settle_kept(kept: &mut Vec<SessionKeptItem>, removed_items: &[DiskItem]) {
    let mut settled: Vec<SessionKeptItem> = Vec::with_capacity(kept.len());
    for entry in kept.drain(..) {
        if removed_items.contains(&entry.kept.item) {
            continue;
        }
        match settled
            .iter_mut()
            .find(|earlier| earlier.kept.item == entry.kept.item)
        {
            Some(earlier) => {
                if earlier.kept.reason == KeepReason::NotCreatedByDelta
                    && entry.kept.reason != KeepReason::NotCreatedByDelta
                {
                    *earlier = entry;
                }
            }
            None => settled.push(entry),
        }
    }
    *kept = settled;
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
