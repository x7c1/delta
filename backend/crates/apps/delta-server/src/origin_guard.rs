//! A CSRF-style guard that rejects requests arriving from a foreign web origin.
//!
//! Delta binds loopback only (`main.rs`), but loopback binding is not a defense
//! on its own: the port is predictable (`DELTA_PORT`, default 7878), and
//! WebSockets are exempt from the browser's same-origin policy and CORS, so any
//! web page the user happens to visit can open `ws://127.0.0.1:7878/pty?…` and
//! get a live read/write PTY into the agent pane. This guard, applied to every
//! route, closes that hole by inspecting two request headers:
//!
//! 1. **`Origin`** — if present, its host must be a loopback host. A browser
//!    sends `Origin` on cross-site requests (and on all WebSocket upgrades), and
//!    a malicious page is served from a real domain, so its `Origin` host is not
//!    loopback and the request is refused. A *present* foreign `Origin` is the
//!    signal the attack leaves behind.
//! 2. **`Host`** — must always be a loopback host. This blocks DNS rebinding,
//!    where a name that resolves to `127.0.0.1` is used to reach the server with
//!    an attacker-controlled `Host`.
//!
//! A **missing `Origin` is allowed** (subject to the `Host` check): the Claude
//! Code hook callbacks are `curl` POSTs to `/hooks/*` with no `Origin`
//! (`delta-bootstrap::settings`), same-origin non-browser clients omit it, and
//! `/health` probes omit it. Rejecting an absent `Origin` would break those, so
//! this is a check on a *present* cross-site `Origin`, not an `Origin`-required
//! check.
//!
//! ## Why loopback-host rather than a fixed origin allowlist
//!
//! Pinning exact origins (e.g. `http://localhost:5173`) is brittle: the app is
//! served by Vite in dev, the `make e2e` / `make e2e-fake` suites boot the
//! backend and browser on dedicated non-default ports (`DELTA_PORT` /
//! `E2E_PORT`), and there is no production static-serving path in this repo. The
//! loopback-host rule blocks the real threat — a remote web origin — without
//! pinning a port, so it holds across dev and the e2e suites alike.

use axum::extract::Request;
use axum::http::{header, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// Rejects a request whose `Host` is not loopback, or that carries a present
/// but non-loopback `Origin`. Everything else is passed through to `next`.
pub(crate) async fn guard(request: Request, next: Next) -> Response {
    let headers = request.headers();

    // `Host` must always be present and loopback (DNS-rebinding defense).
    let host_ok = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .and_then(authority_host)
        .is_some_and(is_loopback_host);
    if !host_ok {
        return StatusCode::FORBIDDEN.into_response();
    }

    // `Origin`, only when present, must be loopback (cross-site CSRF defense).
    if let Some(origin) = headers.get(header::ORIGIN) {
        let origin_ok = origin.to_str().ok().is_some_and(origin_host_is_loopback);
        if !origin_ok {
            return StatusCode::FORBIDDEN.into_response();
        }
    }

    next.run(request).await
}

/// True for the three loopback hosts we accept, on any port.
fn is_loopback_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

/// Extract the host from an authority (`host` or `host:port`), returning the
/// inner address for a bracketed IPv6 literal (`[::1]` / `[::1]:8080` -> `::1`).
///
/// Returns `None` for input that is not a well-formed authority (an empty host,
/// or an unterminated IPv6 bracket), so a malformed value is treated as foreign.
fn authority_host(authority: &str) -> Option<&str> {
    let host = if let Some(rest) = authority.strip_prefix('[') {
        // IPv6 literal: take what is inside the brackets.
        let end = rest.find(']')?;
        &rest[..end]
    } else {
        // `host` or `host:port`: the host runs up to the first colon.
        authority.split(':').next().unwrap_or(authority)
    };
    (!host.is_empty()).then_some(host)
}

/// True when `origin` is a well-formed web origin (`scheme://host[:port]`, no
/// path) with an `http`/`https`/`ws`/`wss` scheme and a loopback host.
///
/// A malformed `Origin` — no scheme separator, an unexpected scheme, or an
/// authority carrying a path — is treated as foreign and returns `false`.
fn origin_host_is_loopback(origin: &str) -> bool {
    let Some((scheme, authority)) = origin.split_once("://") else {
        return false;
    };
    if !matches!(scheme, "http" | "https" | "ws" | "wss") {
        return false;
    }
    // A valid Origin has no path; if one snuck in, `authority_host` keeps it
    // attached to the host and the loopback check below fails it — foreign.
    authority_host(authority).is_some_and(is_loopback_host)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authority_host_extracts_the_host_across_shapes() {
        assert_eq!(authority_host("localhost"), Some("localhost"));
        assert_eq!(authority_host("127.0.0.1:7878"), Some("127.0.0.1"));
        assert_eq!(authority_host("evil.example:443"), Some("evil.example"));
        assert_eq!(authority_host("[::1]"), Some("::1"));
        assert_eq!(authority_host("[::1]:5173"), Some("::1"));
        assert_eq!(authority_host(""), None);
        assert_eq!(authority_host("[::1"), None); // unterminated bracket
    }

    #[test]
    fn origin_host_is_loopback_accepts_only_loopback_web_origins() {
        assert!(origin_host_is_loopback("http://localhost:5173"));
        assert!(origin_host_is_loopback("http://127.0.0.1:7878"));
        assert!(origin_host_is_loopback("https://localhost"));
        assert!(origin_host_is_loopback("ws://127.0.0.1:9999"));
        assert!(origin_host_is_loopback("wss://[::1]:8080"));

        // Foreign host, unexpected scheme, and malformed input are all rejected.
        assert!(!origin_host_is_loopback("https://evil.example"));
        assert!(!origin_host_is_loopback("http://localhost.evil.example"));
        assert!(!origin_host_is_loopback("file:///etc/passwd"));
        assert!(!origin_host_is_loopback("localhost:5173")); // no scheme
        assert!(!origin_host_is_loopback("null"));
        assert!(!origin_host_is_loopback("http://localhost/path")); // path present
    }
}
