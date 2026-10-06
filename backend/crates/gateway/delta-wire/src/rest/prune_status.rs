//! How a closed session ended, as the bulk session removal names it.

use delta_usecase::PruneStatus;
use serde::Deserialize;
use ts_rs::TS;

/// Which closed sessions `POST /api/sessions/prune` takes, by how they ended.
///
/// `ended` is a session whose launch bound and that is now closed — its row
/// may still say `active`, since a row keeps that status after its session
/// closes; open sessions are skipped at removal time. `failed` is a launch that
/// ended without binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename = "PruneStatus")]
pub enum WirePruneStatus {
    Ended,
    Failed,
}

impl WirePruneStatus {
    /// Parse one value of the preview's comma-separated `statuses` query
    /// parameter, spelled as in the JSON body.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "ended" => Some(Self::Ended),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

impl From<WirePruneStatus> for PruneStatus {
    fn from(status: WirePruneStatus) -> Self {
        match status {
            WirePruneStatus::Ended => PruneStatus::Ended,
            WirePruneStatus::Failed => PruneStatus::Failed,
        }
    }
}
