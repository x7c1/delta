//! A response to one of our outgoing requests.

use serde_json::{json, Value};

use super::{RequestId, RpcError};

/// A response to one of our outgoing requests.
#[derive(Debug, Clone, PartialEq)]
pub struct Response {
    /// The correlation id copied from the request.
    pub id: RequestId,
    /// `Ok(result)` for a success (the `result` payload, possibly `Null`),
    /// `Err(error)` for a JSON-RPC error object.
    pub outcome: std::result::Result<Value, RpcError>,
}

impl Response {
    /// Re-render this response as the JSON frame it was parsed from, for the
    /// comms-log inspector.
    ///
    /// Reconstructed rather than kept verbatim: the reader parses each line into
    /// this type and does not hold the original bytes. The reconstruction is
    /// faithful for everything the transport models (`id` plus `result` or
    /// `error`, with the payload preserved as raw JSON); a top-level field the
    /// parser deliberately ignores — a `jsonrpc` version tag — does not
    /// reappear, which is exactly what the inspector should show, since a field
    /// the parser dropped is a field Delta never acted on.
    pub fn to_frame_json(&self) -> String {
        let frame = match &self.outcome {
            Ok(result) => json!({ "id": self.id, "result": result }),
            Err(error) => json!({ "id": self.id, "error": error }),
        };
        frame.to_string()
    }
}
