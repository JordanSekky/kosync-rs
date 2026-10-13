use std::sync::LazyLock;

use axum::{
    extract::{FromRef, FromRequestParts},
    http::request::Parts,
};
use bcrypt::DEFAULT_COST;

use crate::{auth::AuthHeaders, db::Db, error::Error, models::User};

/// Verified against when the username is unknown, so that request takes as
/// long as one with a wrong key and doesn't reveal which accounts exist.
static DUMMY_HASH: LazyLock<String> =
    LazyLock::new(|| bcrypt::hash("", DEFAULT_COST).expect("Failed to hash dummy password"));

/// The user the request's auth headers belong to, with the key checked
/// against the stored hash. Unknown users and wrong keys are both rejected
/// with [`Error::Unauthorized`].
#[derive(Debug, Clone)]
pub struct AuthenticatedUser(pub User);

impl<S> FromRequestParts<S> for AuthenticatedUser
where
    S: Send + Sync,
    Db: FromRef<S>,
{
    type Rejection = Error;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let auth = AuthHeaders::from_request_parts(parts, state).await?;
        let db = Db::from_ref(state);

        let user = User::find_by_username(&db, &auth.username).await?;
        let hash = match &user {
            Some(user) => user.password_hash.clone(),
            None => DUMMY_HASH.clone(),
        };

        // bcrypt is deliberately slow, keep it off the async workers
        let verified = tokio::task::spawn_blocking(move || bcrypt::verify(auth.key, &hash))
            .await
            .map_err(|_| Error::ServerError)??;

        match user {
            Some(user) if verified => Ok(Self(user)),
            _ => Err(Error::Unauthorized),
        }
    }
}
