//! The outcome counts of boot-time re-adoption.

/// What boot-time re-adoption found, one count per outcome, for the boot log.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ReadoptionSummary {
    /// Sessions open again on their surviving pane.
    pub adopted: usize,
    /// Of [`Self::adopted`], those whose agent can no longer deliver hooks to
    /// this server because the hook endpoint changed.
    pub hooks_unreachable: usize,
    /// Sessions whose pane was gone; their record was cleared.
    pub gone: usize,
    /// Sessions tmux could not be asked about; left closed, record kept.
    pub unprobed: usize,
    /// Sessions whose re-adoption failed on a store error, or whose record
    /// named a tmux session another (newer) row already claimed.
    pub failed: usize,
}
