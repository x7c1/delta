//! A session the bulk removal left in place, as `POST /api/sessions/prune`
//! reports it.

use delta_usecase::{SkipReason, SkippedSession};
use serde::Serialize;
use ts_rs::TS;

use super::WireSkipReason;

/// A session that matched the bulk removal's criteria but was not removed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "SkippedSession")]
pub struct WireSkippedSession {
    pub session_id: String,
    pub reason: WireSkipReason,
    /// The error, for `reason: "failed"`; `null` otherwise.
    pub detail: Option<String>,
}

impl From<SkippedSession> for WireSkippedSession {
    fn from(skipped: SkippedSession) -> Self {
        Self {
            session_id: skipped.session_id.as_str().to_owned(),
            reason: WireSkipReason::from(&skipped.reason),
            detail: match skipped.reason {
                SkipReason::Failed(error) => Some(error),
                _ => None,
            },
        }
    }
}
