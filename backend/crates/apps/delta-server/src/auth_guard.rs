//! A per-run bearer-token guard, applied to every route as defense in depth on
//! top of the Origin/Host guard (`crate::origin_guard`).
//!
//! Loopback binding and the Origin/Host guard keep a *foreign web origin* out,
//! but they do not stop a local non-browser process: a `curl` on the same host
//! sends no `Origin` and a loopback `Host`, so the Origin/Host guard passes it
//! through. This guard closes that gap by requiring a secret the server mints
//! (or is handed) once for its lifetime — see `main.rs::config_from_env` and
//! `AppState::token`. Every browser request carries it; anything without the
//! valid token gets `401`.
//!
//! The token arrives in one of two ways, because a browser cannot set request
//! headers on a WebSocket upgrade:
//!
//! 1. **`Authorization: Bearer <token>`** — every REST call (the fetch
//!    chokepoint in `@delta/api-client` sets it).
//! 2. **`token=<token>` query parameter** — the three WebSocket upgrades
//!    (`/ws`, `/pty`, `/comms`), where the frontend appends it in `wsUrl()`.
//!    Read straight off the request URI here, so the socket handlers keep their
//!    existing `Query` structs unchanged.
//!
//! ## Exemptions
//!
//! `/hooks/*` and `/health` are exempt from *this* (bearer) guard by path
//! prefix:
//!
//! - `/hooks/*` is the Claude Code control plane — `curl` POSTs from Claude
//!   Code, not the browser, so there is no place to carry a bearer token. It is
//!   not left unauthenticated, though: it has its own per-run authentication in
//!   [`crate::hook_auth_guard`], which requires the `hs` secret Delta renders
//!   into the hook URLs. This guard defers hooks to that one rather than
//!   demanding a bearer token they cannot carry.
//! - `/health` is an unauthenticated liveness probe (exempt from both guards).
//!
//! ## Middleware order
//!
//! This guard is layered *outside* the Origin/Host guard (see
//! `route_binder::finish`), so it runs **first**: a request is authenticated
//! before its origin is inspected. The pre-existing Origin/Host tests still
//! assert `403` for a foreign origin, so they send a valid token to pass this
//! guard and reach the origin check; a request that reaches the origin check is
//! therefore always one that already presented a valid token.

use axum::extract::{Request, State};
use axum::http::{header, StatusCode, Uri};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::state::AppState;

/// Rejects a request that does not present the per-run token, except on the
/// exempt `/hooks/*` and `/health` paths. Everything else is passed to `next`.
pub(crate) async fn guard(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let path = request.uri().path();
    // Defer the Claude Code control plane and the health probe (see module
    // docs): hooks are `curl`ed by Claude Code with no place to carry a bearer
    // token, so they authenticate through their own `hs` secret in
    // `crate::hook_auth_guard` instead; `/health` is an unauthenticated liveness
    // probe.
    if path == "/health" || path.starts_with("/hooks/") {
        return next.run(request).await;
    }

    let presented = token_from_request(&request);
    let authorized =
        presented.is_some_and(|token| constant_time_eq(token.as_bytes(), state.token().as_bytes()));
    if !authorized {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    next.run(request).await
}

/// The presented token, taken from an `Authorization: Bearer <token>` header
/// (HTTP) or, failing that, a `token=<token>` query parameter (WebSocket
/// upgrades, which cannot set headers).
fn token_from_request(request: &Request) -> Option<&str> {
    if let Some(token) = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
    {
        return Some(token);
    }
    token_from_query(request.uri())
}

/// The `token` query parameter, if present. The token the frontend appends is
/// hex/UUID text (URL-safe), so a plain prefix scan is enough — no percent
/// decoding is needed.
fn token_from_query(uri: &Uri) -> Option<&str> {
    uri.query()?
        .split('&')
        .find_map(|pair| pair.strip_prefix("token="))
}

/// Constant-time byte-slice equality, so a wrong token cannot be recovered by
/// timing the comparison. Returning early on a length mismatch only leaks the
/// length, which is not secret (the token length is fixed for a run).
pub(crate) fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_time_eq_matches_only_equal_slices() {
        assert!(constant_time_eq(b"token", b"token"));
        assert!(!constant_time_eq(b"token", b"tokeN"));
        assert!(!constant_time_eq(b"token", b"tok")); // length mismatch
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn token_from_query_reads_the_token_param() {
        assert_eq!(
            token_from_query(&"/ws?token=abc".parse().unwrap()),
            Some("abc"),
        );
        assert_eq!(
            token_from_query(&"/pty?session_id=s1&token=abc".parse().unwrap()),
            Some("abc"),
        );
        assert_eq!(token_from_query(&"/ws".parse().unwrap()), None);
        assert_eq!(
            token_from_query(&"/ws?session_id=s1".parse().unwrap()),
            None
        );
    }
}
