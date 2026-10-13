use axum::extract::FromRef;
use clap::Parser;
use tokio_util::task::TaskTracker;

mod auth;
mod authenticated_user;
mod config;
mod db;
mod error;
mod handlers;
mod models;
mod upstream;

use config::Config;
use db::Db;
use handlers::RegistrationEnabled;
use upstream::Upstream;

#[derive(Clone, FromRef)]
struct AppState {
    db: Db,
    registration_enabled: RegistrationEnabled,
    /// Only set, and its HTTP client only built, when an upstream URL was given.
    upstream: Option<Upstream>,
    /// Background upstream pushes, waited on at shutdown.
    tasks: TaskTracker,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let config = Config::parse();

    let db = Db::connect(&config.database_url)
        .await
        .expect("Failed to open database");

    let tasks = TaskTracker::new();

    let app = handlers::router().with_state(AppState {
        db: db.clone(),
        registration_enabled: RegistrationEnabled(config.enable_registration),
        upstream: config.upstream_url.map(Upstream::new),
        tasks: tasks.clone(),
    });

    let listener = tokio::net::TcpListener::bind(config.bind_addr)
        .await
        .unwrap_or_else(|e| panic!("Failed to bind to {}: {e}", config.bind_addr));
    // Stops accepting connections on ctrl-c and lets in-flight requests finish
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("Ahhh!");

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
