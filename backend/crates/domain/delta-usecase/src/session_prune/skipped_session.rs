//! A session a bulk removal left in place, and why.

use delta_model::SessionId;

use super::SkipReason;

/// A session that matched the bulk removal's criteria but was not removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedSession {
    pub session_id: SessionId,
    pub reason: SkipReason,
}
