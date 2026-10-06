//! Removing old closed sessions in bulk: what to remove ([`PruneCriteria`])
//! and what happened ([`PruneReport`]). Each session goes through the same
//! removal a single one does, so the rule on the worktree and branch Delta
//! created for it is `SessionContext::delete_session`'s.

mod prune_criteria;
pub use prune_criteria::PruneCriteria;
mod prune_report;
pub use prune_report::PruneReport;
mod prune_status;
pub use prune_status::PruneStatus;
mod session_kept_item;
pub use session_kept_item::SessionKeptItem;
mod skip_reason;
pub use skip_reason::SkipReason;
mod skipped_session;
pub use skipped_session::SkippedSession;
