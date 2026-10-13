use std::net::SocketAddr;

use clap::Parser;
use url::Url;

mod cors;
mod path_prefix;

pub use cors::CorsOrigins;
pub use path_prefix::PathPrefix;

#[derive(Debug, Clone, Parser)]
#[command(version, about)]
pub struct Config {
    /// SQLite database to store users and progress in
    #[arg(long, env = "DATABASE_URL", default_value = "sqlite://kosync.db")]
    pub database_url: String,

    /// Address to listen on
    #[arg(long, env = "BIND_ADDR", default_value = "0.0.0.0:3000")]
    pub bind_addr: SocketAddr,

    /// Address the admin listener (health check and admin API) listens on. The
    /// admin API is not authenticated, so it only listens on loopback by default.
    #[arg(long, env = "ADMIN_BIND_ADDR", default_value = "127.0.0.1:3001")]
    pub admin_bind_addr: SocketAddr,

    /// Path the admin routes are served under, such as `/api`, so the admin
    /// API can share a port with a web client behind a reverse proxy. The
    /// proxy must pass the prefix through. Empty serves them at the root.
    #[arg(
        long,
        env = "ADMIN_PATH_PREFIX",
        default_value = "",
        value_name = "PATH"
    )]
    pub admin_path_prefix: PathPrefix,

    /// Origins browsers may call the admin API from: a comma-separated list
    /// such as `http://localhost:5173`, or `*` for any. Empty, the default,
    /// allows none, so only same-origin pages and non-browser clients can use it.
    #[arg(
        long,
        env = "ADMIN_CORS_ORIGINS",
        default_value = "",
        value_name = "ORIGINS"
    )]
    pub admin_cors_origins: CorsOrigins,

    /// Whether new users can register. Existing users are unaffected.
    #[arg(
        long,
        env = "ENABLE_REGISTRATION",
        default_value_t = true,
        action = clap::ArgAction::Set,
        value_name = "BOOL"
    )]
    pub enable_registration: bool,

    /// Base URL of another kosync implementation
    #[arg(long, env = "UPSTREAM_URL")]
    pub upstream_url: Option<Url>,

    /// How many of the most recent log lines to keep in memory for the admin
    /// log stream to replay to new viewers. 0 keeps none, so a viewer only
    /// sees what is logged while it is connected.
    #[arg(
        long,
        env = "LOG_BUFFER_LINES",
        default_value_t = 1000,
        value_name = "LINES"
    )]
    pub log_buffer_lines: usize,

    /// Print the admin API's OpenAPI document to stdout and exit, without
    /// opening the database or binding any listener
    #[arg(long)]
    pub print_openapi: bool,
}
