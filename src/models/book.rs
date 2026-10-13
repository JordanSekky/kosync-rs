use sqlx::{FromRow, QueryBuilder, Sqlite};

use crate::{db::Db, error::Result};

/// A document that has at least one history entry. Each metadata field is
/// the newest value any user reported for it, so it can come from different
/// entries and users.
#[derive(Debug, Clone, FromRow)]
pub struct Book {
    pub document: String,
    pub filename: Option<String>,
    pub title: Option<String>,
    pub authors: Option<String>,
    /// How many users have synced the document
    pub reader_count: i64,
    pub entry_count: i64,
    /// Unix nanoseconds
    pub last_activity_ns: i64,
}

impl Book {
    /// Every document, most recently synced first, optionally only those one
    /// user has synced.
    pub async fn list(db: &Db, user_id: Option<i64>) -> Result<Vec<Self>> {
        // Metadata fields are optional on every push and independent of each
        // other, so each is the newest entry that has that field. Taking them
        // all from the newest entry would lose a title whenever the last push
        // came from a client that only sent a filename.
        let mut query = QueryBuilder::<Sqlite>::new(
            "SELECT s.document, f.filename, t.title, a.authors,
                    s.reader_count, s.entry_count, s.last_activity_ns
             FROM (
                 SELECT document,
                        COUNT(DISTINCT user_id) AS reader_count,
                        COUNT(*) AS entry_count,
                        MAX(timestamp_ns) AS last_activity_ns,
                        MAX(CASE WHEN filename IS NOT NULL THEN id END) AS filename_id,
                        MAX(CASE WHEN title IS NOT NULL THEN id END) AS title_id,
                        MAX(CASE WHEN authors IS NOT NULL THEN id END) AS authors_id
                 FROM progress_history",
        );
        if let Some(user_id) = user_id {
            query.push(" WHERE user_id = ").push_bind(user_id);
        }
        query.push(
            " GROUP BY document
             ) AS s
             LEFT JOIN progress_history AS f ON f.id = s.filename_id
             LEFT JOIN progress_history AS t ON t.id = s.title_id
             LEFT JOIN progress_history AS a ON a.id = s.authors_id
             ORDER BY s.last_activity_ns DESC, s.document",
        );

        let books = query.build_query_as().fetch_all(&db.read).await?;
        Ok(books)
    }
}
