//! The one-time code handoff: `POST /api/auth/exchange` for every state a code
//! can be in, and `POST /api/auth/codes` behind the bearer guard.
//!
//! Each request carries a loopback `Host`, and none carries a bearer token
//! unless the case is about one, so it is the code that decides the outcome.

use super::*;
use axum::body::{to_bytes, Body};
use axum::http::{header, Request, StatusCode};
use axum::response::Response;
use std::time::Duration;
use tower::ServiceExt;

/// A loopback JSON `POST /api/auth/exchange` with no bearer token.
fn exchange_request() -> axum::http::request::Builder {
    Request::builder()
        .method("POST")
        .uri("/api/auth/exchange")
        .header("host", "127.0.0.1:7878")
        .header("content-type", "application/json")
}

async fn exchange(state: &AppState, body: &str) -> Response {
    router(state.clone())
        .oneshot(
            exchange_request()
                .body(Body::from(body.to_owned()))
                .unwrap(),
        )
        .await
        .unwrap()
}

fn code_body(code: &str) -> String {
    serde_json::json!({ "code": code }).to_string()
}

async fn body_bytes(response: Response) -> Vec<u8> {
    to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap()
        .to_vec()
}

#[tokio::test]
async fn a_valid_code_is_exchanged_for_the_token_once() {
    let state = test_state().await;
    let code = state.mint_one_time_code();

    let first = exchange(&state, &code_body(&code)).await;
    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(
        first.headers().get(header::CACHE_CONTROL).unwrap(),
        "no-store"
    );
    let body: serde_json::Value = serde_json::from_slice(&body_bytes(first).await).unwrap();
    assert_eq!(body, serde_json::json!({ "token": TEST_AUTH_TOKEN }));

    let second = exchange(&state, &code_body(&code)).await;
    assert_eq!(
        second.status(),
        StatusCode::UNAUTHORIZED,
        "a used code is refused"
    );
    let body = body_bytes(second).await;
    assert!(
        !String::from_utf8_lossy(&body).contains(TEST_AUTH_TOKEN),
        "a refused exchange returns no token"
    );
}

#[tokio::test]
async fn an_expired_code_is_refused() {
    let state = test_state()
        .await
        .with_one_time_code_lifetime(Duration::ZERO);
    let code = state.mint_one_time_code();

    let response = exchange(&state, &code_body(&code)).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(body_bytes(response).await.is_empty());
}

#[tokio::test]
async fn an_unknown_code_is_refused() {
    let state = test_state().await;
    state.mint_one_time_code();

    for code in ["not-a-code", "", TEST_AUTH_TOKEN] {
        let response = exchange(&state, &code_body(code)).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{code:?}");
        assert!(body_bytes(response).await.is_empty());
    }
}

#[tokio::test]
async fn a_missing_code_is_a_client_error() {
    let state = test_state().await;
    for body in ["{}", "", "not json"] {
        let response = exchange(&state, body).await;
        assert!(
            response.status().is_client_error(),
            "{body:?} answered {}",
            response.status()
        );
    }
}

#[tokio::test]
async fn a_foreign_origin_cannot_exchange_a_code() {
    // Exempt from the bearer guard, the exchange still sits behind the
    // Origin/Host guard: a foreign page cannot redeem a code it got hold of.
    let state = test_state().await;
    let code = state.mint_one_time_code();
    let body = code_body(&code);

    let response = router(state.clone())
        .oneshot(
            exchange_request()
                .header("origin", "https://evil.example")
                .body(Body::from(body.clone()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    // The refused request did not spend the code.
    let response = exchange(&state, &body).await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn a_loopback_origin_can_exchange_a_code() {
    // What the page sends from the Vite dev server or the server's own origin.
    let state = test_state().await;
    let body = code_body(&state.mint_one_time_code());
    let response = router(state.clone())
        .oneshot(
            exchange_request()
                .header("origin", "http://localhost:5173")
                .body(Body::from(body.clone()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

/// A loopback `POST /api/auth/codes`, with `authorization` when given.
async fn mint(state: &AppState, authorization: Option<String>) -> Response {
    let mut request = Request::builder()
        .method("POST")
        .uri("/api/auth/codes")
        .header("host", "127.0.0.1:7878");
    if let Some(value) = authorization {
        request = request.header("authorization", value);
    }
    router(state.clone())
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn minting_a_code_requires_the_bearer_token() {
    let state = test_state().await;
    for authorization in [None, Some("Bearer not-the-real-token".to_owned())] {
        let response = mint(&state, authorization.clone()).await;
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "{authorization:?}"
        );
    }
}

#[tokio::test]
async fn a_minted_code_opens_the_page_once() {
    let state = test_state().await;
    let response = mint(&state, Some(bearer())).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CACHE_CONTROL).unwrap(),
        "no-store"
    );
    let body: delta_wire::rest::WireAuthCodeResponse =
        serde_json::from_slice(&body_bytes(response).await).unwrap();

    let response = exchange(&state, &code_body(&body.code)).await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test(flavor = "multi_thread")]
async fn open_url_prints_a_url_whose_code_the_running_server_accepts() {
    use crate::open_url::{open_url, OpenUrlArgs};
    use crate::token_file::TokenFile;

    let state = test_state().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = router(state.clone());
    tokio::spawn(async move { axum::serve(listener, app).await });

    let dir = tempfile::tempdir().unwrap();
    let file = TokenFile::write(dir.path().join("auth-token"), TEST_AUTH_TOKEN).unwrap();
    let path = file.path().to_path_buf();
    let args = OpenUrlArgs {
        origin: Some("http://localhost:5173".to_owned()),
    };
    let url = tokio::task::spawn_blocking(move || open_url(&args, path, port))
        .await
        .unwrap()
        .unwrap();

    let code = url
        .strip_prefix("http://localhost:5173/#code=")
        .unwrap_or_else(|| panic!("{url} opens the given origin with a code"));
    let response = exchange(&state, &code_body(code)).await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test(flavor = "multi_thread")]
async fn open_url_reports_a_token_the_running_server_refuses() {
    use crate::open_url::{open_url, OpenUrlArgs, OpenUrlError};
    use crate::token_file::TokenFile;

    let state = test_state().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = router(state);
    tokio::spawn(async move { axum::serve(listener, app).await });

    let dir = tempfile::tempdir().unwrap();
    let file = TokenFile::write(dir.path().join("auth-token"), "from-an-earlier-run").unwrap();
    let path = file.path().to_path_buf();
    let result = tokio::task::spawn_blocking(move || open_url(&OpenUrlArgs::default(), path, port))
        .await
        .unwrap();
    assert!(
        matches!(result, Err(OpenUrlError::TokenRefused { .. })),
        "{result:?}"
    );
}
