use axum::{
    Router,
    extract::FromRef,
    routing::{get, post, put},
};
use tokio_util::task::TaskTracker;

use super::{method_not_allowed, not_found};
use crate::{db::Db, upstream::Upstream};

mod progress;
mod users;

pub use progress::UpdateProgressRequest;
pub use users::RegistrationEnabled;

/// The kosync API KOReader talks to. Works with any state that can hand out
/// a [`Db`], the [`RegistrationEnabled`] setting, the optional [`Upstream`]
/// and a [`TaskTracker`] for background work.
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
