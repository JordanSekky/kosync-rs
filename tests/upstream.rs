mod common;

use std::sync::{Arc, Mutex};

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::put,
};
use common::{TestApp, progress};
use kosync_proxy::upstream::Upstream;
use serde_json::{Value, json};
use tokio::net::TcpListener;
use url::Url;

/// What an upstream server was sent
#[derive(Debug, Clone)]
struct Received {
    path: String,
    headers: HeaderMap,
    body: Value,
}

#[derive(Clone)]
struct FakeUpstream {
    received: Arc<Mutex<Vec<Received>>>,
    /// What to answer pushes with
    status: StatusCode,
}

/// Serves a kosync upstream under `prefix` and returns its base URL.
async fn fake_upstream(prefix: &str, status: StatusCode) -> (Url, FakeUpstream) {
    let fake = FakeUpstream {
        received: Arc::default(),
        status,
    };

    async fn push(
        State(fake): State<FakeUpstream>,
        uri: axum::http::Uri,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> (StatusCode, Json<Value>) {
        fake.received.lock().unwrap().push(Received {
            path: uri.path().to_owned(),
            headers,
            body,
        });
        (fake.status, Json(json!({})))
    }

    let router = Router::new()
        .route(&format!("{prefix}/syncs/progress"), put(push))
        .with_state(fake.clone());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = Url::parse(&format!(
        "http://{}{prefix}",
        listener.local_addr().unwrap()
    ))
    .unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

    (url, fake)
}

/// Waits for the background pushes, as shutdown does
async fn finish_forwards(app: &TestApp) {
    app.state.tasks.close();
    app.state.tasks.wait().await;
}

#[tokio::test]
async fn pushes_are_forwarded_with_the_clients_credentials() {
    let (url, upstream) = fake_upstream("", StatusCode::OK).await;
    let app = TestApp::builder()
        .upstream(Upstream::new(url))
        .build()
        .await;
    app.register("ada").await;

    let response = app.push("ada", progress("book", 0.3, "kindle")).await;
    finish_forwards(&app).await;

    assert_eq!(response.status, StatusCode::OK);
    let received = upstream.received.lock().unwrap();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].path, "/syncs/progress");
    assert_eq!(received[0].headers["x-auth-user"], "ada");
    assert_eq!(received[0].headers["x-auth-key"], common::KEY);
    assert_eq!(
        received[0].headers["accept"],
        "application/vnd.koreader.v1+json"
    );
    assert_eq!(received[0].body["document"], "book");
    assert_eq!(received[0].body["percentage"], 0.3);
    assert_eq!(received[0].body["device"], "kindle");
}

#[tokio::test]
async fn a_path_prefix_on_the_upstream_url_is_kept() {
    let (url, upstream) = fake_upstream("/kosync", StatusCode::OK).await;
    let app = TestApp::builder()
        .upstream(Upstream::new(url))
        .build()
        .await;
    app.register("ada").await;

    app.push("ada", progress("book", 0.3, "kindle")).await;
    finish_forwards(&app).await;

    let received = upstream.received.lock().unwrap();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].path, "/kosync/syncs/progress");
}

#[tokio::test]
async fn a_failing_upstream_does_not_fail_the_push() {
    let (url, upstream) = fake_upstream("", StatusCode::INTERNAL_SERVER_ERROR).await;
    let app = TestApp::builder()
        .upstream(Upstream::new(url))
        .build()
        .await;
    app.register("ada").await;

    let push = app.push("ada", progress("book", 0.3, "kindle")).await;
    finish_forwards(&app).await;

    assert_eq!(push.status, StatusCode::OK);
    assert_eq!(upstream.received.lock().unwrap().len(), 1, "it was tried");
    assert_eq!(app.pull("ada", "book").await.json()["percentage"], 0.3);
}

#[tokio::test]
async fn an_unreachable_upstream_does_not_fail_the_push() {
    // Bound and dropped, so nothing is listening on the port
    let closed = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = Url::parse(&format!("http://{}", closed.local_addr().unwrap())).unwrap();
    drop(closed);
    let app = TestApp::builder()
        .upstream(Upstream::new(url))
        .build()
        .await;
    app.register("ada").await;

    let push = app.push("ada", progress("book", 0.3, "kindle")).await;
    finish_forwards(&app).await;

    assert_eq!(push.status, StatusCode::OK);
}

#[tokio::test]
async fn pulls_and_registrations_are_not_forwarded() {
    let (url, upstream) = fake_upstream("", StatusCode::OK).await;
    let app = TestApp::builder()
        .upstream(Upstream::new(url))
        .build()
        .await;

    app.register("ada").await;
    app.pull("ada", "book").await;
    finish_forwards(&app).await;

    assert!(upstream.received.lock().unwrap().is_empty());
}
