//! How a turn finished.

/// How a turn finished. Mirrors the app-server `turn/completed` status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnStatus {
    /// The turn ran to completion.
    Completed,
    /// The user interrupted the turn before it completed.
    Interrupted,
    /// The turn ended in an error.
    Failed,
}
