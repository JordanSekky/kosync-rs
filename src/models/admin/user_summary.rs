use sqlx::FromRow;

use crate::{db::Db, error::Result};

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
