//! Serving the built web frontend from the server's own origin.
//!
//! In development the SPA is served by Vite, which proxies the API and the live
//! channels to this server. A packaged build has no Vite, so the server hands
//! out the SPA itself: under the `embed-web` feature, [`BUILT`] compiles Vite's
//! build output (`frontend/packages/apps/web/dist/`) into the binary and
//! [`mount`] serves it. The code here is feature-independent — only the real
//! directory is feature-gated — so the route tests drive [`mount`] with a small
//! fixture directory on every `cargo test`.
//!
//! ## What is served
//!
//! - `GET /` and `GET /index.html` — the page, with `Cache-Control: no-cache`
//!   so a rebuilt binary's page (and so its new asset hashes) is picked up.
//! - Any other file in the directory — with a content type from its extension.
//!   Vite's `assets/` files carry a content hash in their names, so they are
//!   served `immutable` with a year-long `max-age`; anything else is `no-cache`.
//!   `mockServiceWorker.js` (MSW's mock-mode worker, copied in from `public/`)
//!   is never served: the packaged app never runs mock mode.
//! - The SPA fallback — any other `GET` that accepts HTML, outside the reserved
//!   prefixes ([`request_scope::RESERVED_PREFIXES`]), gets the page, so a
//!   reload on a deep link lands back on that screen.
//! - A miss — any other `GET`/`HEAD` outside the reserved prefixes, matching
//!   no file and not accepting HTML, such as a browser's automatic
//!   `/favicon.ico` — answers `404` here rather than reaching the bearer
//!   guard. Such a path belongs to the static surface, so a token-less
//!   browser request for it is a plain miss, not an unauthenticated API call.
//!
//! Everything else — non-`GET`/`HEAD` methods and any path under a reserved
//! prefix — is handed to the API router's own fallback, so it answers exactly
//! as it does without this module (`401` without a bearer token, `404` with
//! one).
//!
//! ## Why a fallback, and why outside the bearer guard
//!
//! [`mount`] installs the static surface as the API router's fallback, so it
//! only ever sees a request no endpoint matched: it cannot shadow a route. The
//! fallback is attached after the router's guard layers, so it is not wrapped
//! by them. It must not sit behind the bearer guard, because the page is what
//! delivers the token: a browser loading `/` has no token to present yet. It
//! carries the Origin/Host guard itself, which keeps a DNS-rebinding page from
//! reading the token out of the served HTML.
//!
//! ## How the page learns the token and the API base
//!
//! The frontend reads the per-run token from a `<meta name="delta-auth-token">`
//! tag (`frontend/packages/apps/web/src/config.ts`). In development Vite
//! injects it; here [`mount`] injects the same tag into `index.html` once, at
//! router construction, from [`AppState::token`](crate::AppState::token). The
//! token therefore never appears in a URL or in the built assets. The API base
//! needs nothing: the build leaves `VITE_API_BASE_URL` unset, so the frontend
//! uses same-origin relative paths, which is this server.

mod content_type;
mod index_page;
mod request_scope;

use std::sync::Arc;

use axum::body::{Body, Bytes};
use axum::extract::Request;
use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use include_dir::Dir;
use tower::ServiceExt;

use content_type::content_type;
use request_scope::{accepts_html, is_reserved};

/// Vite's build output for the web app, compiled into the binary.
///
/// The path is resolved at compile time, so building with `embed-web` requires
/// `make web-dist` to have run first; the build fails otherwise.
#[cfg(feature = "embed-web")]
pub(crate) static BUILT: Dir<'static> =
    include_dir::include_dir!("$CARGO_MANIFEST_DIR/../../../../frontend/packages/apps/web/dist");

/// Files present in the build output that are deliberately not served.
const EXCLUDED_FILES: &[&str] = &["mockServiceWorker.js"];

/// `Cache-Control` for Vite's content-hashed `assets/` files.
const IMMUTABLE: &str = "public, max-age=31536000, immutable";

/// `Cache-Control` for everything whose name does not change with its content.
const NO_CACHE: &str = "no-cache";

/// Installs the SPA in `site` as `api`'s fallback, with `token` injected into
/// its page. See the module docs for what is served and why.
///
/// # Panics
///
/// If `site` has no `index.html`, or that page has no `</head>` to inject the
/// token before — a broken build, refused at boot rather than served.
pub(crate) fn mount(api: Router, site: &'static Dir<'static>, token: &str) -> Router {
    let web = Arc::new(StaticWeb {
        site,
        index: index_page::render(site, token),
    });
    // A clone taken before the fallback is installed: its fallback is still
    // the default one, behind the guard layers, so delegating to it answers a
    // request exactly as the router did before this module existed.
    let api_fallback = api.clone();
    let static_surface = Router::new()
        .fallback(move |request: Request| {
            let web = Arc::clone(&web);
            let api_fallback = api_fallback.clone();
            async move { web.respond(request, api_fallback).await }
        })
        .layer(axum::middleware::from_fn(crate::origin_guard::guard));
    api.fallback_service(static_surface)
}

struct StaticWeb {
    site: &'static Dir<'static>,
    /// `index.html` with the token tag injected, rendered once.
    index: Bytes,
}

impl StaticWeb {
    async fn respond(&self, request: Request, api_fallback: Router) -> Response {
        match self.lookup(&request) {
            Some(response) => response,
            // Infallible: a `Router` never fails as a service.
            None => match api_fallback.oneshot(request).await {
                Ok(response) => response,
                Err(never) => match never {},
            },
        }
    }

    /// The static response for `request`, or `None` when it is not ours to
    /// answer (see the module docs).
    fn lookup(&self, request: &Request) -> Option<Response> {
        if !matches!(*request.method(), Method::GET | Method::HEAD) {
            return None;
        }
        let path = request.uri().path();
        if is_reserved(path) {
            return None;
        }
        if path == "/" || path == "/index.html" {
            return Some(self.index());
        }
        let relative = path.strip_prefix('/').unwrap_or(path);
        if let Some(file) = self
            .site
            .get_file(relative)
            .filter(|_| !EXCLUDED_FILES.contains(&relative))
        {
            let cache = if relative.starts_with("assets/") {
                IMMUTABLE
            } else {
                NO_CACHE
            };
            return Some(file_response(
                content_type(relative),
                cache,
                Bytes::from_static(file.contents()),
            ));
        }
        if accepts_html(request.headers()) {
            return Some(self.index());
        }
        Some(StatusCode::NOT_FOUND.into_response())
    }

    fn index(&self) -> Response {
        let mut response = file_response(content_type("index.html"), NO_CACHE, self.index.clone());
        response.headers_mut().insert(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(index_page::CONTENT_SECURITY_POLICY),
        );
        response
    }
}

fn file_response(content_type: &'static str, cache: &'static str, body: Bytes) -> Response {
    (
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, cache),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        Body::from(body),
    )
        .into_response()
}
