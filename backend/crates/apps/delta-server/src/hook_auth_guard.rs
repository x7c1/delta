//! A per-run hook-secret guard, applied to the `/hooks/*` control plane.
//!
//! The Claude Code hooks are `curl`ed (or POSTed by native `http` hooks) from
//! Claude Code, not the browser, so they carry no bearer token and the
//! `crate::auth_guard` bearer guard deliberately defers them. That would leave
//! them reachable by any local process — a `curl` on the same host clears the
//! loopback bind and the Origin/Host guard — which matters because a hook
//! payload names the `transcript_path` Delta then reads from disk and surfaces
//! to the browser. This guard closes that gap: every hook URL Delta renders
//! carries a per-run secret as an `?hs=<secret>` query parameter (see
//! `delta-bootstrap`'s `render_session_settings`), and a request to `/hooks/*`
//! without the matching secret is refused with `401` before its handler runs.
//!
//! The secret rides in the URL, not a header, because Claude Code's native
//! `http` hooks have no proven way to set request headers here; the URL form
//! works uniformly for both the `http` hooks and the `command` (`curl`) hook.
//!
//! ## Scope
//!
//! Only `/hooks/*` is guarded here — every other path (the `/api/*` surface, the
//! WebSocket upgrades, `/health`) passes straight through, since those are the
//! bearer guard's concern. This guard is layered alongside the bearer guard in
//! `route_binder::finish`; the two are independent (a `/hooks/*` request needs
//! the `hs` secret but no bearer token, and an `/api/*` request the reverse).

use axum::extract::{Request, State};
use axum::http::{StatusCode, Uri};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::auth_guard::constant_time_eq;
use crate::state::AppState;

/// Rejects a `/hooks/*` request that does not present the per-run hook secret in
/// its `hs` query parameter. Every other path is passed straight to `next`.
pub(crate) async fn guard(State(state): State<AppState>, request: Request, next: Next) -> Response {
    if !request.uri().path().starts_with("/hooks/") {
        return next.run(request).await;
    }

    let presented = hook_secret_from_query(request.uri());
    let authorized = presented
        .is_some_and(|secret| constant_time_eq(secret.as_bytes(), state.hook_secret().as_bytes()));
    if !authorized {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    next.run(request).await
}

/// The `hs` query parameter, if present. The secret Delta renders is hex text
/// (URL-safe), so a plain prefix scan is enough — no percent decoding is needed,
/// exactly as `auth_guard::token_from_query` reads the WebSocket `token`.
fn hook_secret_from_query(uri: &Uri) -> Option<&str> {
    uri.query()?
        .split('&')
        .find_map(|pair| pair.strip_prefix("hs="))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_hs_param_regardless_of_position() {
        assert_eq!(
            hook_secret_from_query(&"/hooks/stop?hs=abc".parse().unwrap()),
            Some("abc"),
        );
        assert_eq!(
            hook_secret_from_query(&"/hooks/stop?foo=1&hs=abc".parse().unwrap()),
            Some("abc"),
        );
        assert_eq!(
            hook_secret_from_query(&"/hooks/stop".parse().unwrap()),
            None
        );
        assert_eq!(
            hook_secret_from_query(&"/hooks/stop?token=abc".parse().unwrap()),
            None,
        );
    }
}
