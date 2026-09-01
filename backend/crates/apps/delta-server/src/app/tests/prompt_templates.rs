//! Prompt-template routes.

use super::*;
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

#[tokio::test]
async fn prompt_templates_list_is_empty_on_a_fresh_store() {
    let response = router(test_state().await)
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .uri("/api/prompt-templates")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        body["prompt_templates"].as_array().unwrap().len(),
        0,
        "no templates registered yet"
    );
}

#[tokio::test]
async fn create_then_list_update_and_delete_prompt_template() {
    let state = test_state().await;
    let app = router(state);

    // Create one template. The body deliberately carries newlines, which
    // must survive the round trip untouched.
    let create = app
        .clone()
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .method("POST")
                .uri("/api/prompt-templates")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"label":"Merge and log","text":"\nOnce CI is green, merge.\n"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create.status(), StatusCode::CREATED);
    let bytes = to_bytes(create.into_body(), usize::MAX).await.unwrap();
    let created: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let id = created["id"].as_i64().unwrap();
    assert_eq!(created["label"], "Merge and log");
    assert_eq!(
        created["text"], "\nOnce CI is green, merge.\n",
        "the text is stored verbatim, newlines included"
    );
    assert!(created["created_at"]
        .as_str()
        .is_some_and(|s| !s.is_empty()));
    assert_eq!(
        created["updated_at"], created["created_at"],
        "a never-edited template reads as updated when it was created"
    );

    // It now lists.
    let list = app
        .clone()
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .uri("/api/prompt-templates")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = to_bytes(list.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let listed = body["prompt_templates"].as_array().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0]["id"].as_i64().unwrap(), id);

    // Editing replaces both fields in place, keeping the id.
    let update = app
        .clone()
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .method("PATCH")
                .uri(format!("/api/prompt-templates/{id}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"label":"Merge","text":"Merge once green."}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(update.status(), StatusCode::OK);
    let bytes = to_bytes(update.into_body(), usize::MAX).await.unwrap();
    let updated: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(updated["id"].as_i64().unwrap(), id);
    assert_eq!(updated["label"], "Merge");
    assert_eq!(updated["text"], "Merge once green.");
    assert_eq!(
        updated["created_at"], created["created_at"],
        "an edit preserves created_at"
    );

    // The edit is reflected in the list, without adding a row.
    let list = app
        .clone()
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .uri("/api/prompt-templates")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = to_bytes(list.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let listed = body["prompt_templates"].as_array().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0]["label"], "Merge");

    // Delete it.
    let delete = app
        .clone()
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .method("DELETE")
                .uri(format!("/api/prompt-templates/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(delete.status(), StatusCode::NO_CONTENT);

    // Deleting it again is an idempotent no-op, not a 404.
    let delete_again = app
        .clone()
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .method("DELETE")
                .uri(format!("/api/prompt-templates/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(delete_again.status(), StatusCode::NO_CONTENT);

    // The list is empty again.
    let list = app
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .uri("/api/prompt-templates")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = to_bytes(list.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["prompt_templates"].as_array().unwrap().len(), 0);
}

/// A whitespace-only `label` or `text` is a `400`: an unnamed template is
/// unpickable and an empty one inserts nothing. The trim applies to this
/// check only — a `text` that is *surrounded* by whitespace is accepted and
/// stored as written (covered by the round-trip test above).
#[tokio::test]
async fn create_prompt_template_rejects_blank_label_or_text() {
    let app = router(test_state().await);

    for body in [
        r#"{"label":"   ","text":"some text"}"#,
        r#"{"label":"Label","text":"\n\t "}"#,
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .header("host", "127.0.0.1")
                    .header("authorization", super::bearer())
                    .method("POST")
                    .uri("/api/prompt-templates")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "expected a 400 for {body}"
        );
    }
}

/// Editing a template that does not exist is a `404` — unlike the delete,
/// which is a no-op, an edit that silently hit nothing would leave the
/// client showing content the server never stored.
#[tokio::test]
async fn update_prompt_template_of_an_unknown_id_is_404() {
    let response = router(test_state().await)
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .method("PATCH")
                .uri("/api/prompt-templates/9999")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"label":"Label","text":"text"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
