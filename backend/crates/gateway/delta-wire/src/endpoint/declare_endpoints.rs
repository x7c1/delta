//! `declare_endpoints!`: the form a declaration takes. The declarations
//! themselves live in the sibling `table` module, so the list that grows with
//! every new route stays a list.

/// Declares every endpoint once, in both the forms callers need.
///
/// Each entry emits a marker type implementing [`Endpoint`](super::Endpoint)
/// (what the server binds a handler to, and what makes the wire types
/// compile-checked) *and* a row of [`ENDPOINTS`](super::ENDPOINTS) (what a
/// program iterates), so neither can be forgotten when a route is added.
///
/// A declaration spells the method the way a request line does (`GET`), and
/// omits the `request`/`response` clause for a direction that carries no JSON —
/// an empty body, plain text, or a socket upgrade. Write the two clauses in that
/// order: `response` before `request` does not expand (`no rules expected this
/// token`).
macro_rules! declare_endpoints {
    ($(
        $(#[$doc:meta])*
        $name:ident: $method:ident $path:literal
            $(, request = $request:ty)?
            $(, response = $response:ty)?;
    )+) => {
        $(
            $(#[$doc])*
            ///
            #[doc = concat!("`", stringify!($method), " ", $path, "`")]
            #[derive(Debug, Clone, Copy, PartialEq, Eq)]
            pub struct $name;

            impl Endpoint for $name {
                const METHOD: Method = declare_endpoints!(@method $method);
                const PATH: &'static str = $path;
                type Request = declare_endpoints!(@shape $($request)?);
                type Response = declare_endpoints!(@shape $($response)?);
            }
        )+

        /// Every endpoint the server serves, in declaration order.
        ///
        /// This is the whole API surface — the browser's REST calls, the hook
        /// control plane and the streams alike — so reading it tells you what
        /// the server answers to, without reading the server.
        pub const ENDPOINTS: &[EndpointSpec] = &[
            $(
                EndpointSpec {
                    method: declare_endpoints!(@method $method),
                    path: $path,
                    request: declare_endpoints!(@name $($request)?),
                    response: declare_endpoints!(@name $($response)?),
                },
            )+
        ];
    };

    // The request-line spelling a declaration uses, in Rust's casing.
    (@method GET) => { Method::Get };
    (@method POST) => { Method::Post };
    (@method PATCH) => { Method::Patch };
    (@method DELETE) => { Method::Delete };

    // An omitted direction carries no JSON: the unit type, and no name to
    // report in the table.
    (@shape) => { () };
    (@shape $shape:ty) => { $shape };
    (@name) => { None };
    (@name $shape:ty) => { Some(stringify!($shape)) };
}

pub(super) use declare_endpoints;
