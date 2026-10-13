use tokio_util::task::TaskTracker;

use crate::{db::Db, error::Error, upstream::Upstream};
use axum::{
    Router,
    extract::FromRef,
    routing::{get, post, put},
};

mod progress;
mod users;

pub use progress::UpdateProgressRequest;
pub use users::RegistrationEnabled;

/// All routes. Works with any state that can hand out a [`Db`], the
/// [`RegistrationEnabled`] setting, the optional [`Upstream`] and a
/// [`TaskTracker`] for background work.
pub fn router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
    Db: FromRef<S>,
    RegistrationEnabled: FromRef<S>,
    Option<Upstream>: FromRef<S>,
    TaskTracker: FromRef<S>,
{
    Router::new()
        .route("/users/create", post(users::register))
        .route("/users/auth", get(users::authenticate))
        .route("/syncs/progress", put(progress::push))
        .route("/syncs/progress/{document}", get(progress::pull))
        .fallback(not_found)
        .method_not_allowed_fallback(method_not_allowed)
}

async fn not_found() -> Error {
    Error::NotFound
}

async fn method_not_allowed() -> Error {
    Error::MethodNotAllowed
}
