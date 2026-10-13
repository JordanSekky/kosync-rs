//! A real app on a throwaway database, driven through its routers in-process.
#![allow(dead_code)]

use std::sync::atomic::{AtomicU32, Ordering};

use axum::{
    Router,
    body::{self, Body},
    http::{HeaderMap, Method, Request, StatusCode, header},
};
use kosync_proxy::{
    app::{self, AppState},
    config::{CorsOrigins, PathPrefix},
    db::Db,
    handlers::kosync::RegistrationEnabled,
    logs::LogBuffer,
    upstream::Upstream,
};
use serde_json::{Value, json};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tower::ServiceExt;

/// What KOReader sends as both the password and the key: md5 of the password.
pub const KEY: &str = "5f4dcc3b5aa765d61d8327deb882cf99";

pub struct TestApp {
    pub state: AppState,
    /// The sync API
    pub kosync: Router,
    /// The admin API
    pub admin: Router,
}

pub struct TestAppBuilder {
    registration_enabled: bool,
    upstream: Option<Upstream>,
    cors_origins: CorsOrigins,
    path_prefix: PathPrefix,
}

impl TestApp {
    pub async fn new() -> Self {
        Self::builder().build().await
    }

    pub fn builder() -> TestAppBuilder {
        TestAppBuilder {
            registration_enabled: true,
            upstream: None,
            cors_origins: CorsOrigins::Only(Vec::new()),
            path_prefix: PathPrefix::default(),
        }
    }

    pub async fn register(&self, username: &str) -> Response {
        let body = json!({ "username": username, "password": KEY });
        self.kosync(Method::POST, "/users/create", None, Some(body))
            .await
    }

    pub async fn authenticate(&self, username: &str, key: &str) -> Response {
        self.kosync(Method::GET, "/users/auth", Some((username, key)), None)
            .await
    }

    /// Pushes progress as `username`, with the client's usual key
    pub async fn push(&self, username: &str, body: Value) -> Response {
        self.kosync(
            Method::PUT,
            "/syncs/progress",
            Some((username, KEY)),
            Some(body),
        )
        .await
    }

    pub async fn pull(&self, username: &str, document: &str) -> Response {
        self.kosync(
            Method::GET,
            &format!("/syncs/progress/{document}"),
            Some((username, KEY)),
            None,
        )
        .await
    }

    pub async fn kosync(
        &self,
        method: Method,
        uri: &str,
        auth: Option<(&str, &str)>,
        body: Option<Value>,
    ) -> Response {
        let mut request = Request::builder().method(method).uri(uri);
        if let Some((user, key)) = auth {
            request = request
                .header("x-auth-user", user)
                .header("x-auth-key", key);
        }
        send(&self.kosync, with_body(request, body)).await
    }

    pub async fn admin_get(&self, uri: &str) -> Response {
        send(&self.admin, get(uri)).await
    }
}

impl TestAppBuilder {
    pub fn registration_enabled(mut self, enabled: bool) -> Self {
        self.registration_enabled = enabled;
        self
    }

    pub fn upstream(mut self, upstream: Upstream) -> Self {
        self.upstream = Some(upstream);
        self
    }

    pub fn cors_origins(mut self, origins: &str) -> Self {
        self.cors_origins = origins.parse().unwrap();
        self
    }

    pub fn path_prefix(mut self, prefix: &str) -> Self {
        self.path_prefix = prefix.parse().unwrap();
        self
    }

    pub async fn build(self) -> TestApp {
        let state = AppState {
            db: Db::connect(&database_url()).await.unwrap(),
            registration_enabled: RegistrationEnabled(self.registration_enabled),
            upstream: self.upstream,
            tasks: TaskTracker::new(),
            log_buffer: LogBuffer::new(100),
            shutdown: CancellationToken::new(),
        };

        let kosync = app::kosync_router(state.clone());
        let (admin, _) = app::admin_router(state.clone(), &self.cors_origins, &self.path_prefix);

        TestApp {
            state,
            kosync,
            admin,
        }
    }
}

/// A fresh database file in Cargo's scratch directory for tests. A file
/// rather than `:memory:` because the read and write pools must see the same
/// database.
fn database_url() -> String {
    static NEXT: AtomicU32 = AtomicU32::new(0);

    let path = format!(
        "{}/test-{}-{}.db",
        env!("CARGO_TARGET_TMPDIR"),
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed),
    );
    // Left over from an earlier run that had the same process id
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{path}{suffix}"));
    }

    format!("sqlite://{path}")
}

pub struct Response {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

impl Response {
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or_else(|e| {
            panic!(
                "body is not JSON ({e}): {}",
                String::from_utf8_lossy(&self.body)
            )
        })
    }

    pub fn text(&self) -> String {
        String::from_utf8(self.body.clone()).unwrap()
    }
}

pub fn get(uri: &str) -> Request<Body> {
    Request::get(uri).body(Body::empty()).unwrap()
}

fn with_body(request: axum::http::request::Builder, body: Option<Value>) -> Request<Body> {
    match body {
        Some(body) => request
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap(),
        None => request.body(Body::empty()).unwrap(),
    }
}

pub async fn send(router: &Router, request: Request<Body>) -> Response {
    let response = router.clone().oneshot(request).await.unwrap();
    let (parts, body) = response.into_parts();

    Response {
        status: parts.status,
        headers: parts.headers,
        body: body::to_bytes(body, usize::MAX).await.unwrap().to_vec(),
    }
}

/// A push body with the fields the client always sends
pub fn progress(document: &str, percentage: f64, device: &str) -> Value {
    json!({
        "document": document,
        "progress": format!("page {percentage}"),
        "percentage": percentage,
        "device": device,
        "device_id": format!("{device}-id"),
    })
}
