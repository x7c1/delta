//! The built-frontend surface `embed-web` mounts behind the API.
//!
//! These drive [`static_web::mount`] with the fixture directory under
//! `tests/fixtures/web-dist/` instead of the real Vite build, so they run on
//! every `cargo test`, with or without the feature. Each request goes through
//! the full router — guards included — so they also pin how the static
//! surface sits next to the API: outside the bearer guard, inside the
//! Origin/Host guard, and never answering for a reserved prefix.

use super::*;
use crate::app::{api_router, static_web};
use axum::body::{to_bytes, Body};
use axum::http::{header, Request, StatusCode};
use axum::response::Response;
use axum::Router;
use include_dir::{include_dir, Dir};
use tower::ServiceExt;

static FIXTURE: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/tests/fixtures/web-dist");

/// What a browser navigation sends as `Accept`.
const NAVIGATION_ACCEPT: &str = "text/html,application/xhtml+xml,*/*;q=0.8";

async fn mounted() -> Router {
    static_web::mount(api_router(test_state().await), &FIXTURE, TEST_AUTH_TOKEN)
}

/// A loopback GET to `uri` with no token, as a browser's first page load is.
fn get(uri: &str) -> axum::http::request::Builder {
    Request::builder()
        .method("GET")
        .uri(uri)
        .header("host", "127.0.0.1:7878")
}

async fn send(router: Router, request: Request<Body>) -> Response {
    router.oneshot(request).await.unwrap()
}

async fn body_text(response: Response) -> String {
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

fn header_of(response: &Response, name: header::HeaderName) -> &str {
    response
        .headers()
        .get(&name)
        .unwrap_or_else(|| panic!("missing {name}"))
        .to_str()
        .unwrap()
}

/// Asserts `response` is the fixture page with the token tag injected.
async fn assert_is_index(response: Response) {
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        header_of(&response, header::CONTENT_TYPE),
        "text/html; charset=utf-8"
    );
    assert_eq!(header_of(&response, header::CACHE_CONTROL), "no-cache");
    assert!(
        header_of(&response, header::CONTENT_SECURITY_POLICY).contains("frame-ancestors 'none'"),
        "the page carries the CSP header",
    );
    let body = body_text(response).await;
    assert!(body.contains("<title>Delta fixture</title>"), "{body}");
    assert!(
        body.contains(&format!(
            r#"<meta name="delta-auth-token" content="{TEST_AUTH_TOKEN}" /></head>"#
        )),
        "the token tag is injected at the end of <head>: {body}",
    );
}

#[tokio::test]
async fn serves_the_page_at_the_root_and_at_index_html_without_a_token() {
    for uri in ["/", "/index.html"] {
        let response = send(mounted().await, get(uri).body(Body::empty()).unwrap()).await;
        assert_is_index(response).await;
    }
}

#[tokio::test]
async fn serves_a_hashed_asset_as_immutable_javascript() {
    let response = send(
        mounted().await,
        get("/assets/app-abc123.js").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        header_of(&response, header::CONTENT_TYPE),
        "application/javascript"
    );
    assert_eq!(
        header_of(&response, header::CACHE_CONTROL),
        "public, max-age=31536000, immutable"
    );
    assert_eq!(
        header_of(&response, header::X_CONTENT_TYPE_OPTIONS),
        "nosniff"
    );
    assert_eq!(body_text(response).await, "console.log(\"fixture\");\n");
}

#[tokio::test]
async fn serves_a_nested_file_with_its_content_type_and_no_cache() {
    let response = send(
        mounted().await,
        get("/icons/nested/logo.svg").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_of(&response, header::CONTENT_TYPE), "image/svg+xml");
    assert_eq!(header_of(&response, header::CACHE_CONTROL), "no-cache");
}

#[tokio::test]
async fn falls_back_to_the_page_for_a_deep_link_navigation() {
    let response = send(
        mounted().await,
        get("/sessions/xyz")
            .header("accept", NAVIGATION_ACCEPT)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_is_index(response).await;
}

#[tokio::test]
async fn does_not_serve_the_page_to_a_non_html_request_for_an_unknown_path() {
    // Answered as the API router answers any unknown path: 401 with no token,
    // 404 with one — never the page.
    for (authorization, expected) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(bearer()), StatusCode::NOT_FOUND),
    ] {
        let mut request = get("/sessions/xyz").header("accept", "application/json");
        if let Some(value) = authorization {
            request = request.header("authorization", value);
        }
        let response = send(mounted().await, request.body(Body::empty()).unwrap()).await;
        assert_eq!(response.status(), expected);
    }
}

#[tokio::test]
async fn never_serves_the_mock_service_worker() {
    let response = send(
        mounted().await,
        get("/mockServiceWorker.js")
            .header("authorization", bearer())
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn leaves_unknown_paths_under_reserved_prefixes_as_they_were() {
    // Even a navigation-style `Accept` must not turn a reserved path into the
    // page: each one answers exactly as the router without the static surface.
    let hook = format!("/hooks/nope{}", hook_query());
    let cases = [
        ("/api/does-not-exist", Some(bearer())),
        ("/api/does-not-exist", None),
        (hook.as_str(), None),
        ("/hooks/nope", None),
        ("/ws/nope", Some(bearer())),
        ("/health/nope", None),
    ];
    for (uri, authorization) in cases {
        let request = |router: Router| {
            let mut request = get(uri).header("accept", NAVIGATION_ACCEPT);
            if let Some(value) = &authorization {
                request = request.header("authorization", value.as_str());
            }
            send(router, request.body(Body::empty()).unwrap())
        };
        let before = request(api_router(test_state().await)).await;
        let after = request(mounted().await).await;
        assert_eq!(after.status(), before.status(), "{uri} keeps its status");
        assert_ne!(after.status(), StatusCode::OK, "{uri} is not served");
    }
}

#[tokio::test]
async fn leaves_non_get_requests_to_the_api() {
    let response = send(
        mounted().await,
        Request::builder()
            .method("POST")
            .uri("/")
            .header("host", "127.0.0.1:7878")
            .header("authorization", bearer())
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn keeps_the_api_behind_its_bearer_guard() {
    let response = send(
        mounted().await,
        get("/api/providers").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn refuses_the_page_to_a_non_loopback_host_or_a_foreign_origin() {
    // The page carries the token, so a DNS-rebinding host or a foreign page
    // must not be handed it.
    let foreign_host = Request::builder()
        .method("GET")
        .uri("/")
        .header("host", "evil.example")
        .body(Body::empty())
        .unwrap();
    let foreign_origin = get("/sessions/xyz")
        .header("accept", NAVIGATION_ACCEPT)
        .header("origin", "https://evil.example")
        .body(Body::empty())
        .unwrap();
    for request in [foreign_host, foreign_origin] {
        let response = send(mounted().await, request).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}
