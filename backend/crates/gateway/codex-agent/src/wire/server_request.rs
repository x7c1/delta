//! A server-originated request (server → client), such as `*/requestApproval`.

use serde_json::{json, Value};

#[cfg(doc)]
use super::Response;

/// A server-originated request (server → client), such as `*/requestApproval`.
/// It carries an id, so the client is expected to answer it; the C1 transport
/// only routes it (the C2 adapter decides and replies).
#[derive(Debug, Clone, PartialEq)]
pub struct ServerRequest {
    /// The request id, preserved verbatim so a later response echoes it back
    /// with the same JSON type the server used.
    pub id: Value,
    /// The method name (e.g. `item/requestApproval`).
    pub method: String,
    /// The method parameters (`Null` when absent).
    pub params: Value,
}

impl ServerRequest {
    /// Re-render this request as the JSON frame it was parsed from, for the
    /// comms-log inspector. See [`Response::to_frame_json`] for why it is
    /// reconstructed rather than kept verbatim.
    pub fn to_frame_json(&self) -> String {
        json!({ "id": self.id, "method": self.method, "params": self.params }).to_string()
    }
}
