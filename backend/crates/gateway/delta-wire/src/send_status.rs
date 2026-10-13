//! JSON shape of a queued send's correlation status.

use delta_model::SendStatus;
use serde::Serialize;
use ts_rs::TS;

/// JSON shape of a queued send's correlation status.
///
/// Mirrors the domain [`SendStatus`] variant-for-variant; see that type
/// for the semantics of each status. This wire twin carries the serialization
/// concerns the domain type must not know about: the lowercase variant names
/// and the TypeScript export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(rename = "SendStatus")]
pub enum WireSendStatus {
    Queued,
    Dispatched,
    Matched,
    Cancelled,
}

impl From<SendStatus> for WireSendStatus {
    fn from(status: SendStatus) -> Self {
        match status {
            SendStatus::Queued => WireSendStatus::Queued,
            SendStatus::Dispatched => WireSendStatus::Dispatched,
            SendStatus::Matched => WireSendStatus::Matched,
            SendStatus::Cancelled => WireSendStatus::Cancelled,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_status_serializes_lowercase() {
        for (status, expected) in [
            (SendStatus::Queued, "queued"),
            (SendStatus::Dispatched, "dispatched"),
            (SendStatus::Matched, "matched"),
            (SendStatus::Cancelled, "cancelled"),
        ] {
            assert_eq!(
                serde_json::to_value(WireSendStatus::from(status)).unwrap(),
                serde_json::json!(expected),
            );
        }
    }
}
