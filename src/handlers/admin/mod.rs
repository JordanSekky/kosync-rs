use std::sync::Arc;

use aide::{
    axum::{ApiRouter, routing::get_with},
    openapi::{Info, OpenApi, Server},
};
use axum::{Extension, Json, Router, extract::FromRef, routing::get};
use tokio_util::sync::CancellationToken;
use tower_http::cors::{AllowOrigin, Any, CorsLayer};

use super::{method_not_allowed, not_found};
use crate::{
    config::{CorsOrigins, PathPrefix},
    db::Db,
    error::ErrorResponse,
    logs::LogBuffer,
};

mod books;
mod devices;
mod health;
mod logs;
mod progress;
mod sse;
mod users;

/// Routes for the admin listener, which runs on its own port, together with
/// the OpenAPI document describing them. Works with any state that can hand
/// out a [`Db`], the [`LogBuffer`] and the [`CancellationToken`] that ends
/// long-lived streams when the server shuts down.
///
/// The routes are read-only. Browsers may call them from `cors_origins`, so
/// that a web client served elsewhere can.
///
/// Every route, `/health` and `/api.json` included, is served under
/// `path_prefix`. The document keeps the routes' own paths and names the
/// prefix as its server, so a client generated from it doesn't depend on
/// where it ends up deployed.
pub fn router<S>(cors_origins: &CorsOrigins, path_prefix: &PathPrefix) -> (Router<S>, Arc<OpenApi>)
where
    S: Clone + Send + Sync + 'static,
    Db: FromRef<S>,
    LogBuffer: FromRef<S>,
    CancellationToken: FromRef<S>,
{
    let mut api = OpenApi {
        info: Info {
            title: "kosync-proxy admin API".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            ..Info::default()
        },
        servers: if path_prefix.is_empty() {
            Vec::new()
        } else {
            vec![Server {
                url: path_prefix.as_str().to_owned(),
                ..Server::default()
            }]
        },
        ..OpenApi::default()
    };

    let router = ApiRouter::new()
        .api_route(
            "/health",
            get_with(health::health, |op| {
                op.id("health").summary("Liveness check").tag("health")
            }),
        )
        .api_route(
            "/users",
            get_with(users::list, |op| {
                op.id("listUsers")
                    .summary("List registered users")
                    .tag("users")
            }),
        )
        .api_route(
            "/users/{id}",
            get_with(users::get, |op| {
                op.id("getUser")
                    .summary("Get a registered user")
                    .tag("users")
            }),
        )
        .api_route(
            "/progress",
            get_with(progress::list, |op| {
                op.id("listProgress")
                    .summary("List progress history")
                    .description(
                        "Entries across all users, newest first, narrowed by the filters. \
                         Page through them by passing each response's `next_cursor` back as \
                         `cursor`.",
                    )
                    .tag("progress")
            }),
        )
        .api_route(
            "/books",
            get_with(books::list, |op| {
                op.id("listBooks")
                    .summary("List documents with progress history")
                    .tag("books")
            }),
        )
        .api_route(
            "/devices",
            get_with(devices::list, |op| {
                op.id("listDevices")
                    .summary("List devices progress was pushed from")
                    .tag("devices")
            }),
        )
        .api_route(
            "/logs",
            get_with(logs::stream, |op| {
                op.id("streamLogs")
                    .summary("Stream application logs")
                    .description(
                        "Server-sent events: the cached log entries, oldest first, followed by \
                         new ones as they are logged. Use `EventSource` rather than `fetch`, \
                         which reconnects and resumes by itself.",
                    )
                    .tag("logs")
            }),
        )
        // A plain axum route, so the spec doesn't document itself: through
        // aide's `get` it would add the whole OpenAPI schema to the components.
        .route("/api.json", get(serve_api))
        .finish_api_with(&mut api, |api| {
            api.default_response::<Json<ErrorResponse>>()
        });

    let api = Arc::new(api);

    // The layers and fallbacks go on the outer router, so requests outside
    // of the prefix are answered the same way as the ones inside it.
    let router = if path_prefix.is_empty() {
        router
    } else {
        Router::new().nest(path_prefix.as_str(), router)
    };

    let router = router
        .fallback(not_found)
        .method_not_allowed_fallback(method_not_allowed)
        .layer(Extension(api.clone()))
        // Applied last, so it also answers preflights for the routes above
        // and puts its headers on the fallbacks' responses.
        .layer(cors_layer(cors_origins));

    (router, api)
}

fn cors_layer(origins: &CorsOrigins) -> CorsLayer {
    let allow_origin = match origins {
        CorsOrigins::Any => AllowOrigin::any(),
        CorsOrigins::Only(origins) => AllowOrigin::list(origins.iter().cloned()),
    };

    // Only the origin is restricted: the API is read-only and takes no
    // credentials, so which methods and headers a page may use isn't a risk.
    CorsLayer::new()
        .allow_origin(allow_origin)
        .allow_methods(Any)
        .allow_headers(Any)
        .expose_headers(Any)
}

async fn serve_api(Extension(api): Extension<Arc<OpenApi>>) -> Json<Arc<OpenApi>> {
    Json(api)
}
