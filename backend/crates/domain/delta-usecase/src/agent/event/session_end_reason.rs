//! Why a session's underlying agent process ended.

/// Why a session's underlying agent process ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionEndReason {
    /// The session was closed deliberately (the pane/process was torn down).
    Closed,
    /// The agent process exited on its own or was lost unexpectedly.
    ProcessExited,
    /// The session ended in a failure state (e.g. a spawn that never bound).
    Failed,
}
