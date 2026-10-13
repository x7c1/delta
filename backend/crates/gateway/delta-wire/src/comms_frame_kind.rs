//! What kind of message one frame is. Wire twin of [`CommsFrameKind`].

use delta_usecase::CommsFrameKind;
use serde::Serialize;
use ts_rs::TS;

/// What kind of message one frame is. Wire twin of [`CommsFrameKind`].
///
/// A server-originated request is `from_agent` + `request`, not a fourth kind —
/// see the domain type for the full (direction, kind) table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename = "CommsFrameKind")]
pub enum WireCommsFrameKind {
    /// Carries an id and awaits a correlated response.
    Request,
    /// Answers a request, carrying its id.
    Response,
    /// Fire and forget: no id, no answer.
    Notification,
}

impl From<CommsFrameKind> for WireCommsFrameKind {
    fn from(kind: CommsFrameKind) -> Self {
        match kind {
            CommsFrameKind::Request => WireCommsFrameKind::Request,
            CommsFrameKind::Response => WireCommsFrameKind::Response,
            CommsFrameKind::Notification => WireCommsFrameKind::Notification,
        }
    }
}
