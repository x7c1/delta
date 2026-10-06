//! Why the bulk session removal left a matching session in place.

use delta_usecase::SkipReason;
use serde::Serialize;
use ts_rs::TS;

/// Why a session matching `POST /api/sessions/prune`'s criteria was not
/// removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename = "SkipReason")]
pub enum WireSkipReason {
    /// The session is open; close it to remove it.
    Open,
    /// The session is still starting.
    Starting,
    /// Something else removed it in the meantime.
    Gone,
    /// Removing it failed; `detail` carries the error.
    Failed,
}

impl From<&SkipReason> for WireSkipReason {
    fn from(reason: &SkipReason) -> Self {
        match reason {
            SkipReason::Open => Self::Open,
            SkipReason::Starting => Self::Starting,
            SkipReason::Gone => Self::Gone,
            SkipReason::Failed(_) => Self::Failed,
        }
    }
}
