use std::fmt;

use axum::{
    extract::FromRequestParts,
    http::{HeaderMap, request::Parts},
};

use crate::error::Error;

pub const AUTH_USER: &str = "x-auth-user";
pub const AUTH_KEY: &str = "x-auth-key";

/// Credentials from the `x-auth-user` / `x-auth-key` headers. Extracting them
/// only proves the headers were sent, not that they are valid.
#[derive(Clone)]
pub struct AuthHeaders {
    pub username: String,
    /// md5(password) as computed by the client; password-equivalent
    pub key: String,
}

impl<S: Send + Sync> FromRequestParts<S> for AuthHeaders {
    type Rejection = Error;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(Self {
            username: header(&parts.headers, AUTH_USER)?,
            key: header(&parts.headers, AUTH_KEY)?,
        })
    }
}

fn header(headers: &HeaderMap, name: &str) -> Result<String, Error> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or(Error::Unauthorized)
}

// Written by hand so the key can't end up in logs.
impl fmt::Debug for AuthHeaders {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuthHeaders")
            .field("username", &self.username)
            .field("key", &"[redacted]")
            .finish()
    }
}
