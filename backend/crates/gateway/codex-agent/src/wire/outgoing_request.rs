//! An outgoing request (client → server).

use serde::Serialize;
use serde_json::Value;

use super::RequestId;

/// An outgoing request (client → server). Serialised without a `jsonrpc` tag.
#[derive(Debug, Serialize)]
pub struct OutgoingRequest<'a> {
    /// The correlation id; the server echoes it back on the response.
    pub id: RequestId,
    /// The method name (e.g. `initialize`, `thread/start`, `turn/start`).
    pub method: &'a str,
    /// The method parameters, omitted from the wire when `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}
