//! Why a bulk removal left a matching session in place.

/// Why a session that matched the bulk removal's criteria was not removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    /// The session is open; it can be removed once it is closed.
    Open,
    /// The session is still starting.
    Starting,
    /// The session was removed by something else in the meantime.
    Gone,
    /// Removing it failed. Carries the error message.
    Failed(String),
}
