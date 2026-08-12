//! [`EndpointSpec`]: one endpoint declaration as a row of the table.

use super::Method;

/// One endpoint, as a program can iterate it: a row of [`ENDPOINTS`](super::ENDPOINTS).
///
/// The wire types appear here as their declared names rather than as types:
/// every row has this one type, while the shapes differ per endpoint. The names
/// come from the same declaration as the [`Endpoint`](super::Endpoint)
/// associated types, so the two cannot drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EndpointSpec {
    /// The HTTP method the endpoint is served under.
    pub method: Method,
    /// The axum-style path, including its `{param}` segments.
    pub path: &'static str,
    /// Name of the request body's wire type, or `None` when there is no body.
    pub request: Option<&'static str>,
    /// Name of the response body's wire type, or `None` when the endpoint
    /// answers with no JSON.
    pub response: Option<&'static str>,
}
