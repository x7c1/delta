//! One log line per HTTP request, so the calls the server receives can be read
//! back from its log.
//!
//! The layer wraps the whole router — the declared endpoints with their guards,
//! and the static web frontend behind them — so a request the bearer guard
//! refuses (`401`) or the origin guard refuses (`403`) is logged like any
//! other. A WebSocket upgrade is logged as the request that upgraded; the life
//! of the socket after that is not.
//!
//! Each line carries the method, the route template the request matched
//! (axum's `MatchedPath`, e.g. `/api/sessions/{id}/threads`, so lines can be
//! grouped by route; `-` when no declared route matched, as for a static
//! asset), the request's path, the response status and the time to the
//! response in milliseconds.
//!
//! The lines are emitted at `debug` under [`TARGET`], so the default `info`
//! filter keeps them off and `info,delta_server::http=debug` turns on this log
//! alone.
//!
//! Only the path is logged, never the query string or a header: the WebSocket
//! upgrades carry the per-run bearer token in their query (see
//! [`crate::auth_guard`]), the hooks carry their secret there, and other routes
//! carry filesystem paths.

use std::time::Instant;

use axum::extract::{MatchedPath, Request};
use axum::middleware::Next;
use axum::response::Response;

/// The `tracing` target of the request lines.
pub(crate) const TARGET: &str = "delta_server::http";

/// Log `request` once its response is ready. Must be applied with
/// `Router::layer`, which runs after routing, so the matched route is known.
pub(crate) async fn log_request(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map(|matched| matched.as_str().to_owned());
    let started = Instant::now();
    let response = next.run(request).await;
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    tracing::debug!(
        target: TARGET,
        %method,
        route = route.as_deref().unwrap_or("-"),
        path,
        status = response.status().as_u16(),
        elapsed_ms = format_args!("{elapsed_ms:.1}"),
        "request"
    );
    response
}
