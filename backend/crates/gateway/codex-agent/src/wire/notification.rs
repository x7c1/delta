//! A server-originated notification (server → client), such as `item/*` or `turn/*`.

use serde_json::{json, Value};

#[cfg(doc)]
use super::Response;

/// A server-originated notification (server → client), such as `item/*` or
/// `turn/*`.
#[derive(Debug, Clone, PartialEq)]
pub struct Notification {
    /// The method name.
    pub method: String,
    /// The method parameters (`Null` when absent).
    pub params: Value,
}

impl Notification {
    /// Re-render this notification as the JSON frame it was parsed from, for the
    /// comms-log inspector. See [`Response::to_frame_json`] for why it is
    /// reconstructed rather than kept verbatim.
    pub fn to_frame_json(&self) -> String {
        json!({ "method": self.method, "params": self.params }).to_string()
    }
}
