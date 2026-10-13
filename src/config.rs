use std::net::SocketAddr;

use clap::Parser;
use url::Url;

#[derive(Debug, Clone, Parser)]
#[command(version, about)]
pub struct Config {
    /// SQLite database to store users and progress in
    #[arg(long, env = "DATABASE_URL", default_value = "sqlite://kosync.db")]
    pub database_url: String,

    /// Address to listen on
    #[arg(long, env = "BIND_ADDR", default_value = "0.0.0.0:3000")]
    pub bind_addr: SocketAddr,

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
}
