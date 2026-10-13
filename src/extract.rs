use axum::extract::{FromRequest, FromRequestParts};

use crate::error::Error;

/// `axum::Json`, but a bad body is rejected with our own [`Error`], so it
/// gets the same `{"message": ...}` shape as every other failure.
#[derive(FromRequest)]
#[from_request(via(axum::Json), rejection(Error))]
pub struct AppJson<T>(pub T);

/// `axum::extract::Path`, rejecting with our own [`Error`].
#[derive(FromRequestParts)]
#[from_request(via(axum::extract::Path), rejection(Error))]
pub struct AppPath<T>(pub T);
