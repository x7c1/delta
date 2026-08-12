//! The HTTP methods the endpoint table routes on.

/// One HTTP method an [`Endpoint`](super::Endpoint) is served under.
///
/// Deliberately a local enum rather than a web framework's type: the endpoint
/// table is the transport-neutral contract, so this crate stays free of any
/// server dependency. The server maps each variant onto its router's method
/// helper when it mounts the endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Method {
    Get,
    Post,
    Patch,
    Delete,
}

impl Method {
    /// The uppercase HTTP token, as it appears on the request line.
    ///
    /// This is the form both request logs and the prose API docs use, so the
    /// table can name a route the same way a reader would.
    pub const fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Patch => "PATCH",
            Method::Delete => "DELETE",
        }
    }
}
