//! [`route_label`]: the one place a route is turned into prose.

use super::Method;

/// Names one route the way a reader does: `"POST /api/sends"`.
///
/// The one place a route is turned into prose, so a declared route and a route
/// the server mounted are always spelled identically — which is what lets the
/// server's coverage check compare the two sets and name the difference.
pub fn route_label(method: Method, path: &str) -> String {
    format!("{} {path}", method.as_str())
}
