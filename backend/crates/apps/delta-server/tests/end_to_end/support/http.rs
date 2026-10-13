//! Driving the router in-process, the way the browser and hook callers do.

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use axum::Router;
use serde_json::Value;
use tower::ServiceExt;

use super::{AUTH_TOKEN, HOOK_SECRET};

pub(crate) async fn post_json(app: &Router, uri: &str, body: Value) -> (StatusCode, Value) {
    // A hook callback authenticates through the `hs` secret in its URL, not the
    // bearer token, so attach it for the `/hooks/*` paths the driver POSTs.
    let uri = if uri.starts_with("/hooks/") {
        format!("{uri}?hs={HOOK_SECRET}")
    } else {
        uri.to_owned()
    };
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                // A loopback `Host` satisfies the router's origin/host guard,
                // just as the real curl/browser clients do in production.
                .header("host", "127.0.0.1")
                // A valid bearer token clears the per-run auth guard.
                .header("authorization", format!("Bearer {AUTH_TOKEN}"))
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    json_response(response).await
}

pub(crate) async fn get(app: &Router, uri: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(uri)
                .header("host", "127.0.0.1")
                .header("authorization", format!("Bearer {AUTH_TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    json_response(response).await
}

pub(crate) async fn delete(app: &Router, uri: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(uri)
                .header("host", "127.0.0.1")
                .header("authorization", format!("Bearer {AUTH_TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    json_response(response).await
}

/// The session ids `GET /api/sessions` currently lists, so a removal's effect
/// (and a refusal's lack of one) is read off the same surface the navigator is
/// rendered from.
pub(crate) async fn listed_session_ids(app: &Router) -> Vec<String> {
    let (status, body) = get(app, "/api/sessions").await;
    assert_eq!(status, StatusCode::OK);
    body["sessions"]
        .as_array()
        .expect("sessions array")
        .iter()
        .map(|item| {
            item["session"]["id"]
                .as_str()
                .expect("session id")
                .to_owned()
        })
        .collect()
}

pub(crate) async fn json_response(response: axum::response::Response) -> (StatusCode, Value) {
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, value)
}
