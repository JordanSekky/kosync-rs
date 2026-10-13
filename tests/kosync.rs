mod common;

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use common::{KEY, TestApp, progress, send};
use kosync_proxy::{app, handlers::kosync::RegistrationEnabled};
use serde_json::json;

#[tokio::test]
async fn registering_creates_the_user() {
    let app = TestApp::new().await;

    let response = app.register("ada").await;

    assert_eq!(response.status, StatusCode::CREATED);
    assert_eq!(response.json(), json!({ "username": "ada" }));
}

#[tokio::test]
async fn registering_a_taken_username_conflicts() {
    let app = TestApp::new().await;
    app.register("ada").await;

    let response = app.register("ada").await;

    assert_eq!(response.status, StatusCode::CONFLICT);
    assert_eq!(
        response.json(),
        json!({ "message": "User already exists." })
    );
}

#[tokio::test]
async fn registration_can_be_disabled() {
    let app = TestApp::builder().registration_enabled(false).build().await;

    let response = app.register("ada").await;

    assert_eq!(response.status, StatusCode::FORBIDDEN);
    assert_eq!(
        response.json(),
        json!({ "message": "Registration is disabled." })
    );
}

#[tokio::test]
async fn disabling_registration_leaves_existing_users_alone() {
    let enabled = TestApp::new().await;
    enabled.register("ada").await;

    // Same database, registration switched off
    let mut state = enabled.state.clone();
    state.registration_enabled = RegistrationEnabled(false);
    let disabled = app::kosync_router(state);

    let request = Request::get("/users/auth")
        .header("x-auth-user", "ada")
        .header("x-auth-key", KEY)
        .body(Body::empty())
        .unwrap();
    let response = send(&disabled, request).await;

    assert_eq!(response.status, StatusCode::OK);
}

#[tokio::test]
async fn the_right_key_authenticates() {
    let app = TestApp::new().await;
    app.register("ada").await;

    let response = app.authenticate("ada", KEY).await;

    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.json(), json!({ "authorized": "OK" }));
}

#[tokio::test]
async fn a_wrong_key_or_unknown_user_is_unauthorized() {
    let app = TestApp::new().await;
    app.register("ada").await;

    for (user, key) in [("ada", "wrong"), ("nobody", KEY)] {
        let response = app.authenticate(user, key).await;

        assert_eq!(response.status, StatusCode::UNAUTHORIZED, "{user}");
        assert_eq!(response.json(), json!({ "message": "Unauthorized" }));
    }
}

#[tokio::test]
async fn missing_credentials_are_unauthorized() {
    let app = TestApp::new().await;

    let no_headers = app.kosync(Method::GET, "/users/auth", None, None).await;
    let empty_key = app
        .kosync(Method::GET, "/users/auth", Some(("ada", "")), None)
        .await;

    assert_eq!(no_headers.status, StatusCode::UNAUTHORIZED);
    assert_eq!(empty_key.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn pushed_progress_can_be_pulled() {
    let app = TestApp::new().await;
    app.register("ada").await;

    let push = app.push("ada", progress("book", 0.25, "kindle")).await;
    let pull = app.pull("ada", "book").await;

    assert_eq!(push.status, StatusCode::OK);
    let pushed = push.json();
    assert_eq!(pushed["document"], "book");
    assert!(pushed["timestamp"].as_i64().unwrap() > 1_600_000_000);

    assert_eq!(pull.status, StatusCode::OK);
    assert_eq!(
        pull.json(),
        json!({
            "username": "ada",
            "document": "book",
            "progress": "page 0.25",
            "percentage": 0.25,
            "device": "kindle",
            "device_id": "kindle-id",
            "timestamp": pushed["timestamp"],
        })
    );
}

#[tokio::test]
async fn pulling_returns_the_newest_push_from_any_device() {
    let app = TestApp::new().await;
    app.register("ada").await;

    app.push("ada", progress("book", 0.25, "kindle")).await;
    app.push("ada", progress("book", 0.5, "phone")).await;

    let pulled = app.pull("ada", "book").await.json();

    assert_eq!(pulled["percentage"], 0.5);
    assert_eq!(pulled["device"], "phone");
}

#[tokio::test]
async fn pulling_a_document_without_progress_is_an_empty_object() {
    let app = TestApp::new().await;
    app.register("ada").await;

    let response = app.pull("ada", "unknown").await;

    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.json(), json!({}));
}

#[tokio::test]
async fn progress_is_private_to_each_user() {
    let app = TestApp::new().await;
    app.register("ada").await;
    app.register("grace").await;
    app.push("ada", progress("book", 0.25, "kindle")).await;

    let response = app.pull("grace", "book").await;

    assert_eq!(response.json(), json!({}));
}

#[tokio::test]
async fn pushing_and_pulling_require_authentication() {
    let app = TestApp::new().await;
    app.register("ada").await;

    let push = app
        .kosync(
            Method::PUT,
            "/syncs/progress",
            None,
            Some(progress("book", 0.1, "kindle")),
        )
        .await;
    let pull = app
        .kosync(Method::GET, "/syncs/progress/book", None, None)
        .await;
    let wrong_key = app
        .kosync(
            Method::GET,
            "/syncs/progress/book",
            Some(("ada", "wrong")),
            None,
        )
        .await;

    assert_eq!(push.status, StatusCode::UNAUTHORIZED);
    assert_eq!(pull.status, StatusCode::UNAUTHORIZED);
    assert_eq!(wrong_key.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_malformed_body_is_rejected_as_json() {
    let app = TestApp::new().await;
    app.register("ada").await;

    let response = app.push("ada", json!({ "document": "book" })).await;

    assert!(response.status.is_client_error(), "{}", response.status);
    assert!(response.json()["message"].is_string());
}

#[tokio::test]
async fn unknown_routes_and_methods_answer_in_json() {
    let app = TestApp::new().await;

    let missing = app.kosync(Method::GET, "/nope", None, None).await;
    let wrong_method = app.kosync(Method::GET, "/syncs/progress", None, None).await;

    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    assert_eq!(missing.json(), json!({ "message": "Not Found." }));
    assert_eq!(wrong_method.status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(
        wrong_method.json(),
        json!({ "message": "Method Not Allowed." })
    );
}
