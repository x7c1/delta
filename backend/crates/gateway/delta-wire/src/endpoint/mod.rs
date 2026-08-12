//! The endpoint inventory: every route the server serves.
//!
//! The rest of this crate answers "what shapes cross the wire"; this module
//! answers "at which routes". The two together are the whole contract, so
//! reading `delta-wire` alone tells you the entire API surface. The server
//! mounts its handlers through [`ENDPOINTS`] and checks the two sets against
//! each other, so it refuses to boot if it would serve a route nobody declared
//! here, or leave a declaration with nothing serving it.
//!
//! Each declaration produces, from a single point, both faces of one endpoint:
//! the [`Endpoint`] marker a handler is bound to, and the [`EndpointSpec`] row
//! a program iterates. The two can never disagree.
//!
//! A declaration carries the method, the path and the JSON body shapes — not
//! the whole request. Query parameters (`/api/workdir/list?path=`,
//! `?session_id=` on `/pty` and `/comms`), path-parameter types, status codes
//! and error bodies live with the handler and are written up in
//! `docs/guides/api/`. Declaring a shape does not force a handler to speak it
//! either: the check above compares methods and paths only, so a handler that
//! starts taking a different body needs its declaration updated by hand.
//!
//! Nothing here is exported to TypeScript: the browser is handed the shapes,
//! and its own client module decides which of them it calls.

mod declare_endpoints;
mod endpoint_spec;
pub use endpoint_spec::EndpointSpec;
mod method;
pub use method::Method;
mod route_label;
pub use route_label::route_label;
mod table;
// The markers are macro-generated, one per row of `ENDPOINTS`, so re-exporting
// them by name would mean maintaining a second copy of the declaration list —
// exactly the duplication the single declaration point exists to prevent.
pub use table::*;

/// One endpoint, as the compiler sees it.
///
/// Each declaration in [`ENDPOINTS`] also produces a marker type implementing
/// this trait, so a caller that mounts, calls or documents an endpoint names
/// *the declaration* (`endpoint::CreateSend`) instead of repeating its method
/// and path as strings.
///
/// [`Request`](Self::Request) and [`Response`](Self::Response) are the wire
/// types the endpoint speaks — a `Wire*` type for the REST surface, a hook
/// payload for the control plane — which is what makes the association
/// compile-checked: renaming or deleting the referenced type breaks the
/// declaration rather than silently outdating it. `()` means the endpoint
/// carries no JSON in that direction (an empty body, plain text, or a socket
/// upgrade).
pub trait Endpoint {
    /// The HTTP method the endpoint is served under.
    const METHOD: Method;
    /// The axum-style path, including its `{param}` segments.
    const PATH: &'static str;
    /// The JSON request body's wire type, or `()` when there is no body.
    type Request;
    /// The JSON response body's wire type — for a stream endpoint, the shape of
    /// one frame on the socket — or `()` when there is no JSON to read.
    type Response;
}
