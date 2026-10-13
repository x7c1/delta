//! An outgoing notification (client → server): a request with no id.

use serde::Serialize;
use serde_json::Value;

/// An outgoing notification (client → server): a request with no id.
#[derive(Debug, Serialize)]
pub struct OutgoingNotification<'a> {
    /// The method name (e.g. `initialized`).
    pub method: &'a str,
    /// The method parameters, omitted from the wire when `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}
