//! Response for `GET /api/sessions/prune`.

use serde::Serialize;
use ts_rs::TS;

/// Response for `GET /api/sessions/prune`: the sessions
/// `POST /api/sessions/prune` with the same criteria would remove now, oldest
/// first. Open and still-starting sessions are never among them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "PrunePreviewResponse")]
pub struct WirePrunePreviewResponse {
    /// How many sessions would be removed (`session_ids.len()`).
    pub count: u32,
    pub session_ids: Vec<String>,
}

impl WirePrunePreviewResponse {
    /// The preview of `session_ids`.
    pub fn new(session_ids: Vec<String>) -> Self {
        Self {
            count: u32::try_from(session_ids.len()).unwrap_or(u32::MAX),
            session_ids,
        }
    }
}
