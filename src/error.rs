use axum::{Json, http::StatusCode, response::IntoResponse};
use serde::Serialize;
use thiserror::Error;

use crate::models::User;

#[derive(Debug, Error)]
pub enum Error {
    #[error("unauthorized")]
    Unauthorized,
    #[error("registration is disabled")]
    RegistrationDisabled,
    /// The username is already registered; carries the existing user.
    #[error("user {} already exists", .0.username)]
    Conflict(User),
    #[error("internal server error")]
    ServerError,
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error(transparent)]
    Bcrypt(#[from] bcrypt::BcryptError),
    /// The upstream kosync server was unreachable or answered with an error.
    #[error("upstream request failed: {0}")]
    Upstream(#[from] reqwest::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

impl IntoResponse for Error {
    fn into_response(self) -> axum::response::Response {
        #[derive(Serialize)]
        struct ErrorResponse {
            message: String,
        }

        let (status, message) = match &self {
            Error::Unauthorized => (StatusCode::UNAUTHORIZED, "Unauthorized"),
            Error::Conflict(existing_user) => {
                tracing::warn!(%existing_user, "Attempted to create user which already exists");
                (StatusCode::CONFLICT, "User already exists.")
            }
            Error::RegistrationDisabled => (StatusCode::FORBIDDEN, "Registration is disabled."),
            Error::ServerError => (StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error."),
            Error::Database(error) => {
                tracing::error!(%error, "database error");
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error.")
            }
            Error::Upstream(upstream_error) => {
                tracing::error!(%upstream_error, "upstream error");
                (StatusCode::BAD_GATEWAY, "Upstream server error.")
            }
            Error::Bcrypt(bcrypt_error) => {
                tracing::error!(%bcrypt_error, "bcrypt error");
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error.")
            }
        };
        (
            status,
            Json(ErrorResponse {
                message: message.to_owned(),
            }),
        )
            .into_response()
    }
}
