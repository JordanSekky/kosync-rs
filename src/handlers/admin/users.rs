use axum::{Json, extract::State};
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    db::{Db, datetime_from_ns},
    error::{Error, Result},
    extract::AppPath,
    models::UserSummary,
};

#[derive(Serialize, JsonSchema, Debug)]
pub struct UserResponse {
    id: i64,
    username: String,
    created_at: DateTime<Utc>,
    /// Number of progress history entries the user has pushed
    entry_count: i64,
    /// Number of distinct documents the user has synced
    book_count: i64,
    /// When the user last pushed progress; null if they never have
    last_active_at: Option<DateTime<Utc>>,
}

impl From<UserSummary> for UserResponse {
    fn from(user: UserSummary) -> Self {
        Self {
            id: user.id,
            username: user.username,
            created_at: datetime_from_ns(user.created_at_ns),
            entry_count: user.entry_count,
            book_count: user.book_count,
            last_active_at: user.last_active_ns.map(datetime_from_ns),
        }
    }
}

#[derive(Serialize, JsonSchema, Debug)]
pub struct ListUsersResponse {
    /// Oldest account first
    users: Vec<UserResponse>,
}

#[tracing::instrument(skip_all)]
pub async fn list(State(db): State<Db>) -> Result<Json<ListUsersResponse>> {
    let users = UserSummary::list(&db).await?;

    Ok(Json(ListUsersResponse {
        users: users.into_iter().map(Into::into).collect(),
    }))
}

/// A struct rather than a bare `i64` so the spec can name the parameter.
#[derive(Deserialize, JsonSchema, Debug)]
pub struct UserPath {
    id: i64,
}

#[tracing::instrument(skip_all, fields(user_id = id))]
pub async fn get(
    State(db): State<Db>,
    AppPath(UserPath { id }): AppPath<UserPath>,
) -> Result<Json<UserResponse>> {
    let user = UserSummary::find(&db, id).await?.ok_or(Error::NotFound)?;

    Ok(Json(user.into()))
}
