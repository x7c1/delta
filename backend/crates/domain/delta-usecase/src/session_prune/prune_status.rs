//! Which closed sessions a bulk removal takes, by how they ended.

use delta_model::SessionStatus;

/// How a closed session ended, as the bulk removal's status choice names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PruneStatus {
    /// The session ran — its launch bound — and is now closed.
    ///
    /// Matches the rows whose status is `active` as well as `ended`: a row
    /// stays `active` after its session closes (open-ness is process-runtime
    /// state, not a column), so `active` alone says nothing about whether the
    /// session is still running. Open sessions are told apart at removal
    /// time and skipped.
    Ended,
    /// The launch ended without ever binding (`failed`).
    Failed,
}

impl PruneStatus {
    /// Both choices: what a request that names no status means.
    pub const ALL: [Self; 2] = [Self::Ended, Self::Failed];

    /// The row statuses this choice covers.
    pub fn row_statuses(self) -> &'static [SessionStatus] {
        match self {
            Self::Ended => &[SessionStatus::Active, SessionStatus::Ended],
            Self::Failed => &[SessionStatus::Failed],
        }
    }
}
