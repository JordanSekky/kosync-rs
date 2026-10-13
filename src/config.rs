use std::{fmt, net::SocketAddr, str::FromStr};

use axum::http::HeaderValue;
use clap::Parser;
use url::Url;

/// A URL path every admin route is served under, so that the admin API can
/// share a host and port with other things behind a reverse proxy.
///
/// Stored normalized: either empty, meaning no prefix, or a leading slash
/// followed by segments and no trailing slash, such as `/admin/api`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PathPrefix(String);

impl PathPrefix {
    /// The prefix without a trailing slash, or "" when there is none.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Display for PathPrefix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if self.0.is_empty() { "/" } else { &self.0 })
    }
}

impl FromStr for PathPrefix {
    type Err = String;

    /// Accepts `api`, `/api`, `/api/` and `/admin/api`; empty and `/` mean no
    /// prefix.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let trimmed = s.trim().trim_matches('/');
        if trimmed.is_empty() {
            return Ok(Self::default());
        }

        for segment in trimmed.split('/') {
            if segment.is_empty() {
                return Err(format!("invalid path prefix `{s}`: empty path segment"));
            }
            if matches!(segment, "." | "..") {
                return Err(format!("invalid path prefix `{s}`: `{segment}` segment"));
            }
            // Kept to characters that mean the same thing literally, in a
            // route pattern and in a URL, so the prefix matches what the
            // proxy forwards byte for byte.
            let valid = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '~');
            if let Some(c) = segment.chars().find(|&c| !valid(c)) {
                return Err(format!(
                    "invalid path prefix `{s}`: unsupported character `{c}`"
                ));
            }
        }

        Ok(Self(format!("/{trimmed}")))
    }
}

/// Which origins the admin listener lets browsers call it from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CorsOrigins {
    /// Any origin, written `*`
    Any,
    /// Only these origins, normalized to how browsers send them. Empty means
    /// no cross-origin access at all.
    Only(Vec<HeaderValue>),
}

impl FromStr for CorsOrigins {
    type Err = String;

    /// Either `*`, or a comma-separated list of origins such as
    /// `http://localhost:5173,https://admin.example.com`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let entries: Vec<&str> = s
            .split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .collect();

        if entries.contains(&"*") {
            return match entries.len() {
                1 => Ok(Self::Any),
                _ => Err("`*` can't be combined with other origins".to_owned()),
            };
        }

        let mut origins = Vec::with_capacity(entries.len());
        for entry in entries {
            let url = Url::parse(entry).map_err(|e| format!("invalid origin `{entry}`: {e}"))?;

            // Browsers send the scheme, host and port and nothing else, so
            // anything more specific than that could never match.
            let is_bare = url.origin().is_tuple()
                && url.path() == "/"
                && url.query().is_none()
                && url.fragment().is_none()
                && url.username().is_empty()
                && url.password().is_none();
            if !is_bare {
                return Err(format!(
                    "invalid origin `{entry}`: expected scheme://host[:port] with no path"
                ));
            }

            // Serializing from the parsed form drops default ports and
            // lowercases the host, as browsers do.
            let origin = url.origin().ascii_serialization();
            let value = HeaderValue::from_str(&origin)
                .map_err(|e| format!("invalid origin `{entry}`: {e}"))?;
            if !origins.contains(&value) {
                origins.push(value);
            }
        }

        Ok(Self::Only(origins))
    }
}

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

    /// Print the admin API's OpenAPI document to stdout and exit, without
    /// opening the database or binding any listener
    #[arg(long)]
    pub print_openapi: bool,
}
