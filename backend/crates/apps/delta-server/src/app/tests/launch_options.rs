//! Launch-option routes.

use super::*;
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

/// Read `GET /api/launch-options` off an app.
async fn list_launch_options(app: &axum::Router) -> Vec<serde_json::Value> {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .uri("/api/launch-options")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    body["launch_options"].as_array().unwrap().clone()
}

/// A fresh store already lists Delta's shipped options — the boot-time
/// reconcile in the composition root has materialized every declared preset
/// — and every one of them is flagged `builtin`, with `default_enabled` off
/// so nothing is imposed.
#[tokio::test]
async fn launch_options_list_holds_only_the_shipped_options_on_a_fresh_store() {
    let app = router(test_state().await);
    let options = list_launch_options(&app).await;

    assert_eq!(
        options.len(),
        delta_bootstrap::all_launch_option_presets().len(),
        "a fresh store lists exactly the declared catalog: {options:?}"
    );
    for option in &options {
        assert_eq!(option["builtin"], serde_json::json!(true));
        assert_eq!(
            option["default_enabled"],
            serde_json::json!(false),
            "a shipped option is offered, not imposed: {option}"
        );
    }
}

#[tokio::test]
async fn create_then_list_and_delete_launch_option() {
    let state = test_state().await;
    let app = router(state);
    let shipped = list_launch_options(&app).await.len();

    // Create one option.
    let create = app
        .clone()
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .method("POST")
                .uri("/api/launch-options")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"label":"plugins","name":"--plugin-dir","value":"/opt/p"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create.status(), StatusCode::CREATED);
    let bytes = to_bytes(create.into_body(), usize::MAX).await.unwrap();
    let created: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let id = created["id"].as_i64().unwrap();
    assert_eq!(created["name"], "--plugin-dir");
    assert_eq!(
        created["builtin"],
        serde_json::json!(false),
        "a row the user registered is never flagged as Delta's"
    );

    // It now lists, after the shipped block.
    let options = list_launch_options(&app).await;
    assert_eq!(options.len(), shipped + 1);
    assert_eq!(
        options.last().unwrap()["id"].as_i64(),
        Some(id),
        "the user's own rows follow every shipped row: {options:?}"
    );

    // Delete it.
    let delete = app
        .clone()
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .method("DELETE")
                .uri(format!("/api/launch-options/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(delete.status(), StatusCode::NO_CONTENT);

    // Only the shipped rows are left.
    assert_eq!(list_launch_options(&app).await.len(), shipped);
}

/// `DELETE` on a row Delta ships is a `409` and the row survives, while the
/// same call against the user's own row is still a `204` — and an id nobody
/// has is still an idempotent `204` no-op.
///
/// The Settings UI omits the delete control on a shipped row entirely, so
/// this answers a stale list; what matters is that the refusal is a
/// refusal, not a silent success.
#[tokio::test]
async fn delete_refuses_a_shipped_launch_option_but_not_the_users_own() {
    let app = router(test_state().await);

    let delete = |id: i64| {
        let app = app.clone();
        async move {
            app.oneshot(
                Request::builder()
                    .header("host", "127.0.0.1")
                    .header("authorization", super::bearer())
                    .method("DELETE")
                    .uri(format!("/api/launch-options/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
        }
    };

    let shipped_id = list_launch_options(&app).await[0]["id"].as_i64().unwrap();
    let refused = delete(shipped_id).await;
    assert_eq!(refused.status(), StatusCode::CONFLICT);
    let bytes = to_bytes(refused.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        body["code"], "launch_option_builtin",
        "every 409 this API returns names its case with a stable code"
    );
    assert!(
        list_launch_options(&app)
            .await
            .iter()
            .any(|option| option["id"].as_i64() == Some(shipped_id)),
        "the refused row is still registered"
    );

    // A row the user registered still goes.
    let create = app
        .clone()
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .method("POST")
                .uri("/api/launch-options")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"--plugin-dir","value":"/opt/p"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = to_bytes(create.into_body(), usize::MAX).await.unwrap();
    let created: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let own_id = created["id"].as_i64().unwrap();
    assert_eq!(delete(own_id).await.status(), StatusCode::NO_CONTENT);

    // And an unknown id stays a no-op.
    assert_eq!(delete(9999).await.status(), StatusCode::NO_CONTENT);
}

/// `PATCH` still works on a row Delta ships: ticking `default_enabled` on a
/// shipped option is the whole point of shipping it, so the endpoint that
/// carries only that flag must not be gated the way `DELETE` is.
#[tokio::test]
async fn patch_flips_default_enabled_on_a_shipped_launch_option() {
    let app = router(test_state().await);
    let shipped = &list_launch_options(&app).await[0];
    let id = shipped["id"].as_i64().unwrap();
    assert_eq!(shipped["default_enabled"], serde_json::json!(false));

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .method("PATCH")
                .uri(format!("/api/launch-options/{id}"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"default_enabled":true}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["default_enabled"], serde_json::json!(true));
    assert_eq!(
        body["builtin"],
        serde_json::json!(true),
        "it is still Delta's row, just ticked"
    );

    let listed = list_launch_options(&app).await;
    assert_eq!(listed[0]["default_enabled"], serde_json::json!(true));
}

/// POST a launch option and hand back `(status, body)`.
async fn post_launch_option(app: &axum::Router, body: &str) -> (StatusCode, serde_json::Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .method("POST")
                .uri("/api/launch-options")
                .header("content-type", "application/json")
                .body(Body::from(body.to_owned()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

/// PATCH a launch option's `default_enabled` and hand back `(status, body)`.
async fn patch_default_enabled(
    app: &axum::Router,
    id: i64,
    default_enabled: bool,
) -> (StatusCode, serde_json::Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .method("PATCH")
                .uri(format!("/api/launch-options/{id}"))
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"default_enabled":{default_enabled}}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

/// An option that turns the agent's own permission system off may be
/// registered, but never pre-checked: a create asking for both is a `400`
/// `launch_option_rejected`, while the same option undefaulted is created and
/// listed — flagged `dangerous` so the browser can mark it.
///
/// The rule lives in the use case, so this pins the whole route honouring it:
/// the composition root wiring the real per-provider predicate, the use case
/// refusing, and the error mapping naming the case with its stable code.
#[tokio::test]
async fn create_rejects_creating_a_dangerous_option_as_default_enabled() {
    let app = router(test_state().await);

    let (status, body) = post_launch_option(
        &app,
        r#"{"name":"--dangerously-skip-permissions","default_enabled":true}"#,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        body["code"], "launch_option_rejected",
        "the refusal names its case with a stable code: {body}"
    );
    assert!(
        !list_launch_options(&app)
            .await
            .iter()
            .any(|option| option["name"] == "--dangerously-skip-permissions"),
        "a refused create registers nothing"
    );

    // Undefaulted, the very same option is registered — and marked.
    let (status, created) =
        post_launch_option(&app, r#"{"name":"--dangerously-skip-permissions"}"#).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["dangerous"], serde_json::json!(true));
    assert_eq!(created["default_enabled"], serde_json::json!(false));

    let listed = list_launch_options(&app).await;
    let row = listed
        .iter()
        .find(|option| option["name"] == "--dangerously-skip-permissions")
        .expect("the undefaulted dangerous option is registered");
    assert_eq!(row["dangerous"], serde_json::json!(true));
    // A benign shipped row is not marked, so the flag is a verdict and not a
    // constant.
    assert!(
        listed
            .iter()
            .any(|option| option["name"] == "--model" && option["dangerous"] == false),
        "a benign shipped row is not marked: {listed:?}"
    );
}

/// `PATCH` cannot turn `default_enabled` on for a dangerous option — including
/// a Codex one whose danger is buried in a `config` value — but turning it off
/// always works, which is how a row registered before this rule is disarmed.
#[tokio::test]
async fn patch_rejects_enabling_default_for_a_dangerous_option() {
    let app = router(test_state().await);

    let (status, created) = post_launch_option(
        &app,
        r#"{"name":"config","value":"{\"sandbox_mode\": \"danger-full-access\"}","provider":"codex"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        created["dangerous"],
        serde_json::json!(true),
        "a `config` row stating the full-access sandbox is dangerous: {created}"
    );
    let id = created["id"].as_i64().unwrap();

    let (status, body) = patch_default_enabled(&app, id, true).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "launch_option_rejected");
    assert!(
        list_launch_options(&app)
            .await
            .iter()
            .any(|option| option["id"].as_i64() == Some(id) && option["default_enabled"] == false),
        "the refused PATCH left the row undefaulted"
    );

    // Disabling is never refused, even on a dangerous row.
    let (status, body) = patch_default_enabled(&app, id, false).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["default_enabled"], serde_json::json!(false));

    // And an id nobody has is still a 404 rather than the new refusal: there is
    // no row to classify.
    let (status, _) = patch_default_enabled(&app, 9999, true).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn create_launch_option_rejects_a_blank_name() {
    let response = router(test_state().await)
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1")
                .header("authorization", super::bearer())
                .method("POST")
                .uri("/api/launch-options")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"   "}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
