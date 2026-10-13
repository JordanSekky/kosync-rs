//! The application as a whole: its shared state, the routers of both
//! listeners, and running them until shutdown.

use std::{net::SocketAddr, sync::Arc};

use aide::openapi::OpenApi;
use axum::{Router, extract::FromRef};
use tokio::net::TcpListener;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use crate::{
    config::{Config, CorsOrigins, PathPrefix},
    db::Db,
    handlers::{self, kosync::RegistrationEnabled},
    logs::LogBuffer,
    upstream::Upstream,
};

#[derive(Clone, FromRef)]
pub struct AppState {
    pub db: Db,
    pub registration_enabled: RegistrationEnabled,
    /// Only set, and its HTTP client only built, when an upstream URL was given.
    pub upstream: Option<Upstream>,
    /// Background upstream pushes, waited on at shutdown.
    pub tasks: TaskTracker,
    /// The most recent log lines, and the feed of new ones
    pub log_buffer: LogBuffer,
    /// Cancelled when shutdown starts, to end streams that would otherwise
    /// keep their connections open and hold up graceful shutdown.
    pub shutdown: CancellationToken,
}

/// The router of the sync API, the one KOReader talks to.
pub fn kosync_router(state: AppState) -> Router {
    handlers::kosync::router().with_state(state)
}

/// The router of the admin listener, and the OpenAPI document describing it.
pub fn admin_router(
    state: AppState,
    cors_origins: &CorsOrigins,
    path_prefix: &PathPrefix,
) -> (Router, Arc<OpenApi>) {
    let (router, api) = handlers::admin::router::<AppState>(cors_origins, path_prefix);
    (router.with_state(state), api)
}

/// The admin API's OpenAPI document as pretty-printed JSON. Needs no state, so
/// it works without a database or a listener.
pub fn openapi_json(config: &Config) -> String {
    let (_, api) =
        handlers::admin::router::<AppState>(&config.admin_cors_origins, &config.admin_path_prefix);
    serde_json::to_string_pretty(&*api).expect("Failed to serialize the OpenAPI document")
}

/// Opens the database, serves both listeners and returns once a shutdown
/// signal has been handled and everything has been wound down.
pub async fn run(config: Config, log_buffer: LogBuffer) {
    let db = Db::connect(&config.database_url)
        .await
        .expect("Failed to open database");

    let tasks = TaskTracker::new();
    let shutdown = CancellationToken::new();

    let state = AppState {
        db: db.clone(),
        registration_enabled: RegistrationEnabled(config.enable_registration),
        upstream: config.upstream_url.map(Upstream::new),
        tasks: tasks.clone(),
        log_buffer,
        shutdown: shutdown.clone(),
    };

    let app = kosync_router(state.clone());
    let (admin_app, _) = admin_router(state, &config.admin_cors_origins, &config.admin_path_prefix);

    let listener = bind(config.bind_addr).await;
    let admin_listener = bind(config.admin_bind_addr).await;

    // One signal stops both listeners, which then let in-flight requests finish
    tokio::spawn({
        let shutdown = shutdown.clone();
        async move {
            shutdown_signal().await;
            shutdown.cancel();
        }
    });

    tokio::join!(
        serve(listener, app, shutdown.clone()),
        serve(admin_listener, admin_app, shutdown),
    );

    // No request can spawn a forward any more, so let the running ones finish
    tasks.close();
    if !tasks.is_empty() {
        tracing::info!(
            pending = tasks.len(),
            "Waiting for in-flight upstream pushes"
        );
    }
    tasks.wait().await;

    db.close().await;
}

async fn bind(addr: SocketAddr) -> TcpListener {
    TcpListener::bind(addr)
        .await
        .unwrap_or_else(|e| panic!("Failed to bind to {addr}: {e}"))
}

async fn serve(listener: TcpListener, app: Router, shutdown: CancellationToken) {
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown.cancelled_owned())
        .await
        .expect("Failed to serve requests");
}

/// Resolves on ctrl-c or, on unix, SIGTERM, which is what `docker stop` and
/// systemd send.
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to listen for ctrl-c");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to listen for SIGTERM")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => tracing::info!("Received ctrl-c, shutting down"),
        _ = terminate => tracing::info!("Received SIGTERM, shutting down"),
    }
}
