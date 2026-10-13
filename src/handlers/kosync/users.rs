use axum::{Json, extract::State, http::StatusCode};
use bcrypt::DEFAULT_COST;

use crate::{
    auth::AuthenticatedUser,
    db::Db,
    error::{Error, Result},
    extract::AppJson,
    models::User,
    protocol::{AuthResponse, CreateUserRequest, CreateUserResponse},
};

/// Whether `POST /users/create` accepts new users. A newtype so it can be
/// pulled out of the app state with `FromRef` without claiming `bool`.
#[derive(Debug, Clone, Copy)]
pub struct RegistrationEnabled(pub bool);

#[tracing::instrument(skip_all, fields(username = %request.username))]
pub async fn register(
    State(db): State<Db>,
    State(RegistrationEnabled(enabled)): State<RegistrationEnabled>,
    AppJson(request): AppJson<CreateUserRequest>,
) -> Result<(StatusCode, Json<CreateUserResponse>)> {
    if !enabled {
        return Err(Error::RegistrationDisabled);
    }

    let hashed = bcrypt::hash(request.password, DEFAULT_COST)?;

    User::create(&db, &request.username, &hashed).await?;

    // The client only treats 201 as a successful registration
    Ok((
        StatusCode::CREATED,
        Json(CreateUserResponse {
            username: request.username,
        }),
    ))
}

#[tracing::instrument(skip_all, fields(user_id = user.id))]
pub async fn authenticate(
    AuthenticatedUser(user): AuthenticatedUser,
) -> Result<Json<AuthResponse>> {
    Ok(Json(AuthResponse::default()))
}
