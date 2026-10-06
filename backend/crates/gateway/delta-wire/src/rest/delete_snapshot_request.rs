//! Request body for `DELETE /api/storage/snapshots`.

use serde::Deserialize;
use ts_rs::TS;

/// Request body for `DELETE /api/storage/snapshots`: delete one of the
/// migration snapshots `GET /api/storage` lists.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[ts(rename = "DeleteSnapshotRequest")]
pub struct WireDeleteSnapshotRequest {
    /// The snapshot's path, exactly as `GET /api/storage` lists it.
    pub path: String,
}
