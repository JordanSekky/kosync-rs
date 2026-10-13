use crate::error::Error;

pub mod admin;
pub mod kosync;

// Shared by the routers of every listener, so unknown routes look the same
// on all of them.

async fn not_found() -> Error {
    Error::NotFound
}

async fn method_not_allowed() -> Error {
    Error::MethodNotAllowed
}
