use aide::OperationIo;
use axum::{
    Json,
    extract::rejection::{JsonRejection, PathRejection, QueryRejection},
    http::StatusCode,
    response::IntoResponse,
};
use schemars::JsonSchema;
use serde::Serialize;
use thiserror::Error;

/// The body of every error response.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ErrorResponse {
    pub message: String,
}

#[derive(Debug, Error, OperationIo)]
#[aide(output)]
pub enum Error {
    #[error("unauthorized")]
    Unauthorized,
    #[error("registration is disabled")]
    RegistrationDisabled,
    /// The username is already registered.
    #[error("user {username} already exists")]
    Conflict { username: String },
    #[error("not found")]
    NotFound,
    #[error("method not allowed")]
    MethodNotAllowed,
    /// The request body wasn't valid JSON of the expected shape.
    #[error("invalid request body: {0}")]
    InvalidJson(#[from] JsonRejection),
    /// A path parameter couldn't be parsed.
    #[error("invalid path: {0}")]
    InvalidPath(#[from] PathRejection),
    /// The query string couldn't be parsed.
    #[error("invalid query: {0}")]
    InvalidQuery(#[from] QueryRejection),
    #[error("internal server error")]
    Server,
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
        let (status, message): (StatusCode, String) = match &self {
            Error::InvalidJson(rejection) => {
                tracing::debug!(%rejection, "rejected request body");
                (rejection.status(), rejection.body_text())
            }
            Error::InvalidPath(rejection) => {
                tracing::debug!(%rejection, "rejected path parameter");
                (rejection.status(), rejection.body_text())
            }
            Error::InvalidQuery(rejection) => {
                tracing::debug!(%rejection, "rejected query string");
                (rejection.status(), rejection.body_text())
            }
            Error::NotFound => (StatusCode::NOT_FOUND, "Not Found.".into()),
            Error::MethodNotAllowed => {
                (StatusCode::METHOD_NOT_ALLOWED, "Method Not Allowed.".into())
            }
            Error::Unauthorized => (StatusCode::UNAUTHORIZED, "Unauthorized".into()),
            Error::Conflict { username } => {
                tracing::warn!(%username, "Attempted to create user which already exists");
                (StatusCode::CONFLICT, "User already exists.".into())
            }
            Error::RegistrationDisabled => {
                (StatusCode::FORBIDDEN, "Registration is disabled.".into())
            }
            Error::Server => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal Server Error.".into(),
            ),
            Error::Database(error) => {
                tracing::error!(%error, "database error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal Server Error.".into(),
                )
            }
            Error::Upstream(upstream_error) => {
                tracing::error!(%upstream_error, "upstream error");
                (StatusCode::BAD_GATEWAY, "Upstream server error.".into())
            }
            Error::Bcrypt(bcrypt_error) => {
                tracing::error!(%bcrypt_error, "bcrypt error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal Server Error.".into(),
                )
            }
        };
        (status, Json(ErrorResponse { message })).into_response()
    }
}
