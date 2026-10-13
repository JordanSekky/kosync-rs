use std::fmt;

use sqlx::FromRow;

use crate::{
    db::{Db, now_ns},
    error::{Error, Result},
};

#[derive(Clone, FromRow)]
pub struct User {
    pub id: i64,
    pub username: String,
    /// bcrypt hash of the client-sent key (md5(password)) combined with the
    /// server-side salt. Verify with bcrypt, never compare directly.
    pub password_hash: String,
    /// Unix nanoseconds
    pub created_at_ns: i64,
}

// Written by hand so the password hash can't end up in logs.
impl fmt::Debug for User {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("User")
            .field("id", &self.id)
            .field("username", &self.username)
            .field("password_hash", &"[redacted]")
            .field("created_at_ns", &self.created_at_ns)
            .finish()
    }
}

impl fmt::Display for User {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (id {})", self.username, self.id)
    }
}

impl User {
    /// Fails with [`Error::Conflict`] if the username is already taken.
    pub async fn create(db: &Db, username: &str, password_hash: &str) -> Result<Self> {
        let mut tx = db.write.begin().await?;

        let created: Option<Self> = sqlx::query_as(
            "INSERT INTO users (username, password_hash, created_at_ns) VALUES (?, ?, ?)
             ON CONFLICT (username) DO NOTHING
             RETURNING id, username, password_hash, created_at_ns",
        )
        .bind(username)
        .bind(password_hash)
        .bind(now_ns())
        .fetch_optional(&mut *tx)
        .await?;

        let Some(user) = created else {
            let existing = sqlx::query_as(
                "SELECT id, username, password_hash, created_at_ns FROM users WHERE username = ?",
            )
            .bind(username)
            .fetch_one(&mut *tx)
            .await?;
            return Err(Error::Conflict(existing));
        };

        tx.commit().await?;
        Ok(user)
    }

    pub async fn find_by_username(db: &Db, username: &str) -> Result<Option<Self>> {
        let user = sqlx::query_as(
            "SELECT id, username, password_hash, created_at_ns FROM users WHERE username = ?",
        )
        .bind(username)
        .fetch_optional(&db.read)
        .await?;
        Ok(user)
    }
}
