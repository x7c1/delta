//! The auto-compact re-dispatch debounce: at most one re-dispatch per compact
//! event, whichever of the two driving paths fires first.

use std::time::{Duration, Instant};

use super::SessionRuntime;

/// How long after an auto-compact re-dispatch fires for a session before
/// another re-dispatch may run for the same session.
///
/// Two paths drive auto-compact re-dispatch — the live
/// `SessionStart(source=compact)` hook and the ingestion-time
/// `Effect::AutoCompactFinished` from the same compaction summary line — and
/// on a live session both can land within a single tick. Without a debounce
/// each `Dispatched` send would be re-typed twice, producing a spurious
/// double submission. Set generously above the gap between the hook and the
/// ingest (the hook fires when Claude finishes compacting; the tail ingests
/// the summary line on the next poll) but well under any plausible interval
/// between distinct compactions.
pub const AUTO_COMPACT_REDISPATCH_DEBOUNCE: Duration = Duration::from_secs(2);

impl SessionRuntime {
    /// Try to claim a window for an auto-compact re-dispatch as of `now`,
    /// returning `true` when the caller should proceed and `false` when a
    /// recent re-dispatch already covered the same compact event.
    ///
    /// On `true` the stamp is updated to `now`. The debounce window is
    /// [`AUTO_COMPACT_REDISPATCH_DEBOUNCE`]; see the field docstring on
    /// [`Self::last_auto_compact_redispatch_at`] for why both the hook path
    /// and the ingestion-effect path key on the same stamp.
    pub(in crate::interactor) fn try_claim_auto_compact_redispatch(
        &mut self,
        now: Instant,
    ) -> bool {
        let stale = self
            .last_auto_compact_redispatch_at
            .map(|t| now.duration_since(t) >= AUTO_COMPACT_REDISPATCH_DEBOUNCE)
            .unwrap_or(true);
        if stale {
            self.last_auto_compact_redispatch_at = Some(now);
        }
        stale
    }
}
