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

/// A user along with a summary of their history, for the admin API. Carries
/// no password hash.
#[derive(Debug, Clone, FromRow)]
pub struct UserSummary {
    pub id: i64,
    pub username: String,
    /// Unix nanoseconds
    pub created_at_ns: i64,
    pub entry_count: i64,
    /// Distinct documents the user has synced
    pub book_count: i64,
    /// Unix nanoseconds of the newest history entry, if there is one
    pub last_active_ns: Option<i64>,
}

impl UserSummary {
    /// Every user, oldest account first.
    pub async fn list(db: &Db) -> Result<Vec<Self>> {
        let users = sqlx::query_as(
            "SELECT u.id, u.username, u.created_at_ns,
                    COUNT(p.id) AS entry_count,
                    COUNT(DISTINCT p.document) AS book_count,
                    MAX(p.timestamp_ns) AS last_active_ns
             FROM users AS u
             LEFT JOIN progress_history AS p ON p.user_id = u.id
             GROUP BY u.id
             ORDER BY u.id",
        )
        .fetch_all(&db.read)
        .await?;
        Ok(users)
    }

    pub async fn find(db: &Db, id: i64) -> Result<Option<Self>> {
        let user = sqlx::query_as(
            "SELECT u.id, u.username, u.created_at_ns,
                    COUNT(p.id) AS entry_count,
                    COUNT(DISTINCT p.document) AS book_count,
                    MAX(p.timestamp_ns) AS last_active_ns
             FROM users AS u
             LEFT JOIN progress_history AS p ON p.user_id = u.id
             WHERE u.id = ?
             GROUP BY u.id",
        )
        .bind(id)
        .fetch_optional(&db.read)
        .await?;
        Ok(user)
    }
}
