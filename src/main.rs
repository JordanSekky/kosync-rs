use std::net::SocketAddr;

use axum::{Router, extract::FromRef};
use clap::Parser;
use tokio::net::TcpListener;
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tracing_subscriber::{
    EnvFilter, filter::LevelFilter, layer::SubscriberExt, util::SubscriberInitExt,
};

mod auth;
mod authenticated_user;
mod config;
mod db;
mod error;
mod extract;
mod handlers;
mod logs;
mod models;
mod upstream;

use config::Config;
use db::Db;
use handlers::kosync::RegistrationEnabled;
use logs::LogBuffer;
use upstream::Upstream;

#[derive(Clone, FromRef)]
struct AppState {
    db: Db,
    registration_enabled: RegistrationEnabled,
    /// Only set, and its HTTP client only built, when an upstream URL was given.
    upstream: Option<Upstream>,
    /// Background upstream pushes, waited on at shutdown.
    tasks: TaskTracker,
    /// The most recent log lines, and the feed of new ones
    log_buffer: LogBuffer,
    /// Cancelled when shutdown starts, to end streams that would otherwise
    /// keep their connections open and hold up graceful shutdown.
    shutdown: CancellationToken,
}

#[tokio::main]
async fn main() {
    let config = Config::parse();

    let log_buffer = LogBuffer::new(config.log_buffer_lines);
    tracing_subscriber::registry()
        .with(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .with(tracing_subscriber::fmt::layer())
        .with(log_buffer.clone())
        .init();

    if config.print_openapi {
        let (_, api) = handlers::admin::router::<AppState>(
            &config.admin_cors_origins,
            &config.admin_path_prefix,
        );
        println!(
            "{}",
            serde_json::to_string_pretty(&*api).expect("Failed to serialize the OpenAPI document")
        );
        return;
    }

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

    let app = handlers::kosync::router().with_state(state.clone());

    let (admin_app, _) =
        handlers::admin::router(&config.admin_cors_origins, &config.admin_path_prefix);
    let admin_app = admin_app.with_state(state);

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
        .expect("Ahhh!");
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
