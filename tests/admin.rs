mod common;

use std::time::Duration;

use axum::http::{Request, StatusCode, header};
use common::{TestApp, get, progress, send};
use futures_util::StreamExt;
use serde_json::{Value, json};
use tokio::time::timeout;
use tower::ServiceExt;
use tracing_subscriber::layer::SubscriberExt;

/// Two users with a few pushes between them, oldest first:
///
/// 1. ada, `dune`, kindle, 0.1
/// 2. ada, `dune`, phone, 0.4, with a title and authors
/// 3. grace, `dune`, kindle, 0.9, with only a filename
/// 4. grace, `emma`, kindle, 0.2
async fn seeded() -> TestApp {
    let app = TestApp::new().await;
    app.register("ada").await;
    app.register("grace").await;

    app.push("ada", progress("dune", 0.1, "kindle")).await;

    let mut with_title = progress("dune", 0.4, "phone");
    with_title["metadata"] = json!({ "title": "Dune", "authors": "Frank Herbert" });
    app.push("ada", with_title).await;

    let mut with_filename = progress("dune", 0.9, "kindle");
    with_filename["metadata"] = json!({ "filename": "dune.epub" });
    app.push("grace", with_filename).await;

    app.push("grace", progress("emma", 0.2, "kindle")).await;
    app
}

fn ids(entries: &Value) -> Vec<i64> {
    entries
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["id"].as_i64().unwrap())
        .collect()
}

#[tokio::test]
async fn health_answers_ok() {
    let app = TestApp::new().await;

    let response = app.admin_get("/health").await;

    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.json(), json!({ "status": "ok" }));
}

#[tokio::test]
async fn users_are_listed_with_their_activity() {
    let app = seeded().await;

    let response = app.admin_get("/users").await;

    assert_eq!(response.status, StatusCode::OK);
    let body = response.json();
    let users = body["users"].as_array().unwrap();
    assert_eq!(users.len(), 2);

    assert_eq!(users[0]["username"], "ada");
    assert_eq!(users[0]["entry_count"], 2);
    assert_eq!(users[0]["book_count"], 1);
    assert!(users[0]["last_active_at"].is_string());

    assert_eq!(users[1]["username"], "grace");
    assert_eq!(users[1]["entry_count"], 2);
    assert_eq!(users[1]["book_count"], 2);
    assert!(
        !users[0].to_string().contains("password"),
        "no hash in the response"
    );
}

#[tokio::test]
async fn a_user_without_history_has_no_last_activity() {
    let app = TestApp::new().await;
    app.register("ada").await;

    let users = app.admin_get("/users").await.json();

    assert_eq!(users["users"][0]["entry_count"], 0);
    assert_eq!(users["users"][0]["last_active_at"], Value::Null);
}

#[tokio::test]
async fn one_user_can_be_fetched_by_id() {
    let app = seeded().await;
    let id = app.admin_get("/users").await.json()["users"][1]["id"]
        .as_i64()
        .unwrap();

    let found = app.admin_get(&format!("/users/{id}")).await;
    let missing = app.admin_get("/users/9999").await;
    let invalid = app.admin_get("/users/abc").await;

    assert_eq!(found.status, StatusCode::OK);
    assert_eq!(found.json()["username"], "grace");
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    assert_eq!(missing.json(), json!({ "message": "Not Found." }));
    assert_eq!(invalid.status, StatusCode::BAD_REQUEST);
    assert!(invalid.json()["message"].is_string());
}

#[tokio::test]
async fn progress_history_is_listed_newest_first() {
    let app = seeded().await;

    let body = app.admin_get("/progress").await.json();

    let entries = &body["entries"];
    let ids = ids(entries);
    assert_eq!(ids.len(), 4);
    assert!(ids.windows(2).all(|pair| pair[0] > pair[1]), "{ids:?}");
    assert_eq!(entries[0]["username"], "grace");
    assert_eq!(entries[0]["document"], "emma");
    assert_eq!(body["next_cursor"], Value::Null);
}

#[tokio::test]
async fn progress_history_is_paged_with_a_cursor() {
    let app = seeded().await;

    let first = app.admin_get("/progress?limit=3").await.json();
    let cursor = first["next_cursor"].as_i64().expect("more pages");
    let second = app
        .admin_get(&format!("/progress?limit=3&cursor={cursor}"))
        .await
        .json();

    assert_eq!(ids(&first["entries"]).len(), 3);
    assert_eq!(cursor, *ids(&first["entries"]).last().unwrap());
    assert_eq!(ids(&second["entries"]).len(), 1);
    assert!(ids(&second["entries"])[0] < cursor);
    assert_eq!(second["next_cursor"], Value::Null);
}

#[tokio::test]
async fn progress_history_can_be_filtered() {
    let app = seeded().await;

    let by_document = app.admin_get("/progress?document=emma").await.json();
    let by_device = app.admin_get("/progress?device=phone").await.json();
    let by_device_id = app.admin_get("/progress?device_id=kindle-id").await.json();
    let combined = app
        .admin_get("/progress?document=dune&device=kindle")
        .await
        .json();
    let future = app
        .admin_get("/progress?from=2999-01-01T00:00:00Z")
        .await
        .json();
    let past = app
        .admin_get("/progress?to=2000-01-01T00:00:00Z")
        .await
        .json();

    assert_eq!(ids(&by_document["entries"]).len(), 1);
    assert_eq!(ids(&by_device["entries"]).len(), 1);
    assert_eq!(ids(&by_device_id["entries"]).len(), 3);
    assert_eq!(ids(&combined["entries"]).len(), 2);
    assert!(ids(&future["entries"]).is_empty());
    assert!(ids(&past["entries"]).is_empty());
}

#[tokio::test]
async fn progress_history_can_be_filtered_by_user() {
    let app = seeded().await;
    let users = app.admin_get("/users").await.json();
    let ada = users["users"][0]["id"].as_i64().unwrap();

    let body = app
        .admin_get(&format!("/progress?user_id={ada}"))
        .await
        .json();

    let entries = body["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    assert!(entries.iter().all(|entry| entry["username"] == "ada"));
}

#[tokio::test]
async fn a_bad_query_is_rejected_as_json() {
    let app = TestApp::new().await;

    let response = app.admin_get("/progress?limit=lots").await;

    assert_eq!(response.status, StatusCode::BAD_REQUEST);
    assert!(response.json()["message"].is_string());
}

#[tokio::test]
async fn books_combine_readers_and_the_newest_metadata_of_each_field() {
    let app = seeded().await;

    let body = app.admin_get("/books").await.json();

    let books = body["books"].as_array().unwrap();
    assert_eq!(books.len(), 2);

    // Most recently synced first
    assert_eq!(books[0]["document"], "emma");
    assert_eq!(books[0]["reader_count"], 1);
    assert_eq!(books[0]["title"], Value::Null);

    // The title and authors come from ada's push and the filename from
    // grace's, even though grace's was the newest
    assert_eq!(books[1]["document"], "dune");
    assert_eq!(books[1]["reader_count"], 2);
    assert_eq!(books[1]["entry_count"], 3);
    assert_eq!(books[1]["title"], "Dune");
    assert_eq!(books[1]["authors"], "Frank Herbert");
    assert_eq!(books[1]["filename"], "dune.epub");
}

#[tokio::test]
async fn metadata_is_carried_forward_to_later_pushes() {
    let app = TestApp::new().await;
    app.register("ada").await;
    let mut first = progress("dune", 0.1, "kindle");
    first["metadata"] = json!({ "title": "Dune" });
    app.push("ada", first).await;
    app.push("ada", progress("dune", 0.2, "kindle")).await;

    let body = app.admin_get("/progress").await.json();

    assert_eq!(body["entries"][0]["title"], "Dune", "the newest entry");
    assert_eq!(body["entries"][1]["title"], "Dune");
}

#[tokio::test]
async fn books_and_devices_can_be_narrowed_to_one_user() {
    let app = seeded().await;
    let users = app.admin_get("/users").await.json();
    let ada = users["users"][0]["id"].as_i64().unwrap();

    let books = app.admin_get(&format!("/books?user_id={ada}")).await.json();
    let devices = app
        .admin_get(&format!("/devices?user_id={ada}"))
        .await
        .json();

    assert_eq!(books["books"].as_array().unwrap().len(), 1);
    assert_eq!(books["books"][0]["document"], "dune");
    assert_eq!(devices["devices"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn devices_are_listed_with_their_use() {
    let app = seeded().await;

    let body = app.admin_get("/devices").await.json();

    let devices = body["devices"].as_array().unwrap();
    assert_eq!(devices.len(), 2);
    assert_eq!(devices[0]["device"], "kindle");
    assert_eq!(devices[0]["device_id"], "kindle-id");
    assert_eq!(devices[0]["entry_count"], 3);
    assert_eq!(devices[1]["device"], "phone");
    assert_eq!(devices[1]["entry_count"], 1);
}

#[tokio::test]
async fn the_openapi_document_describes_every_route() {
    let app = TestApp::new().await;

    let response = app.admin_get("/api.json").await;

    assert_eq!(response.status, StatusCode::OK);
    let api = response.json();
    let paths = api["paths"].as_object().unwrap();
    for path in [
        "/health",
        "/users",
        "/users/{id}",
        "/progress",
        "/books",
        "/devices",
        "/logs",
    ] {
        assert!(paths.contains_key(path), "{path} is missing");
    }
    assert!(
        api.get("servers")
            .is_none_or(|servers| servers.is_null() || servers.as_array().unwrap().is_empty())
    );
}

#[tokio::test]
async fn a_path_prefix_moves_every_route_and_names_the_server() {
    let app = TestApp::builder().path_prefix("/admin/api").build().await;

    let health = app.admin_get("/admin/api/health").await;
    let spec = app.admin_get("/admin/api/api.json").await;
    let outside = app.admin_get("/health").await;

    assert_eq!(health.status, StatusCode::OK);
    assert_eq!(spec.json()["servers"][0]["url"], "/admin/api");
    assert!(
        spec.json()["paths"].get("/health").is_some(),
        "routes keep their own paths in the document"
    );
    assert_eq!(outside.status, StatusCode::NOT_FOUND);
    assert_eq!(outside.json(), json!({ "message": "Not Found." }));
}

#[tokio::test]
async fn only_configured_origins_are_allowed_by_cors() {
    let app = TestApp::builder()
        .cors_origins("http://localhost:5173")
        .build()
        .await;

    let allowed = send(
        &app.admin,
        Request::get("/health")
            .header(header::ORIGIN, "http://localhost:5173")
            .body(axum::body::Body::empty())
            .unwrap(),
    )
    .await;
    let other = send(
        &app.admin,
        Request::get("/health")
            .header(header::ORIGIN, "http://evil.test")
            .body(axum::body::Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(
        allowed.headers[header::ACCESS_CONTROL_ALLOW_ORIGIN],
        "http://localhost:5173"
    );
    assert!(
        !other
            .headers
            .contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN)
    );
}

#[tokio::test]
async fn unknown_admin_routes_answer_in_json() {
    let app = TestApp::new().await;

    let response = app.admin_get("/nope").await;

    assert_eq!(response.status, StatusCode::NOT_FOUND);
    assert_eq!(response.json(), json!({ "message": "Not Found." }));
}

#[tokio::test]
async fn logs_stream_the_cached_entries_and_end_at_shutdown() {
    let app = TestApp::new().await;
    {
        let subscriber = tracing_subscriber::registry().with(app.state.log_buffer.clone());
        let _guard = tracing::subscriber::set_default(subscriber);
        tracing::info!(user = "ada", "something happened");
    }

    let response = app.admin.clone().oneshot(get("/logs")).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "text/event-stream"
    );
    assert_eq!(response.headers()["x-accel-buffering"], "no");

    let mut body = response.into_body().into_data_stream();
    let first = timeout(Duration::from_secs(5), body.next())
        .await
        .expect("the cached entry arrives without waiting")
        .expect("the stream is open")
        .unwrap();

    let first = String::from_utf8(first.to_vec()).unwrap();
    assert!(first.contains("id: "), "{first}");
    let data = first
        .lines()
        .find_map(|line| line.strip_prefix("data: "))
        .expect("an event");
    let entry: Value = serde_json::from_str(data).unwrap();
    assert_eq!(entry["message"], "something happened");
    assert_eq!(entry["level"], "info");
    assert_eq!(entry["fields"]["user"], "ada");

    // It stays open until the server shuts down, which graceful shutdown
    // would otherwise wait on for as long as a page is open
    app.state.shutdown.cancel();
    let end = timeout(Duration::from_secs(5), async {
        while body.next().await.is_some() {}
    })
    .await;
    assert!(end.is_ok(), "the stream ends at shutdown");
}
