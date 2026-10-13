use aide::OperationIo;
use axum::extract::{FromRequest, FromRequestParts};

use crate::error::Error;

/// `axum::Json`, but a bad body is rejected with our own [`Error`], so it
/// gets the same `{"message": ...}` shape as every other failure.
#[derive(FromRequest)]
#[from_request(via(axum::Json), rejection(Error))]
pub struct AppJson<T>(pub T);

/// `axum::extract::Path`, rejecting with our own [`Error`].
///
/// Documented in the OpenAPI spec exactly as `Path` is.
#[derive(FromRequestParts, OperationIo)]
#[from_request(via(axum::extract::Path), rejection(Error))]
#[aide(input_with = "axum::extract::Path<T>", json_schema)]
pub struct AppPath<T>(pub T);

/// `axum::extract::Query`, rejecting with our own [`Error`].
///
/// Documented in the OpenAPI spec exactly as `Query` is.
#[derive(FromRequestParts, OperationIo)]
#[from_request(via(axum::extract::Query), rejection(Error))]
#[aide(input_with = "axum::extract::Query<T>", json_schema)]
pub struct AppQuery<T>(pub T);
