use sqlx::{FromRow, QueryBuilder, Sqlite};

use crate::{db::Db, error::Result};

/// A history entry along with the name of the user it belongs to.
#[derive(Debug, Clone, FromRow)]
pub struct ProgressEntry {
    pub id: i64,
    pub user_id: i64,
    pub username: String,
    pub document: String,
    pub progress: String,
    pub percentage: f64,
    pub device: String,
    pub device_id: Option<String>,
    pub filename: Option<String>,
    pub title: Option<String>,
    pub authors: Option<String>,
    /// Server clock at write time, unix nanoseconds
    pub timestamp_ns: i64,
}

/// Narrows [`ProgressEntry::list`]; every field that is set must match.
#[derive(Debug, Default, Clone, Copy)]
pub struct ProgressFilter<'a> {
    pub user_id: Option<i64>,
    pub document: Option<&'a str>,
    pub device: Option<&'a str>,
    pub device_id: Option<&'a str>,
    /// Inclusive, unix nanoseconds
    pub from_ns: Option<i64>,
    /// Exclusive, unix nanoseconds
    pub to_ns: Option<i64>,
}

impl ProgressEntry {
    /// Entries matching the filter, newest first. `before_id` continues a
    /// previous page: only entries with a smaller id are returned.
    pub async fn list(
        db: &Db,
        filter: &ProgressFilter<'_>,
        before_id: Option<i64>,
        limit: i64,
    ) -> Result<Vec<Self>> {
        // Built up condition by condition so an absent filter adds nothing to
        // the query and SQLite is free to pick the index for the others.
        let mut query = QueryBuilder::<Sqlite>::new(
            "SELECT p.id, p.user_id, u.username, p.document, p.progress, p.percentage,
                    p.device, p.device_id, p.filename, p.title, p.authors, p.timestamp_ns
             FROM progress_history AS p
             JOIN users AS u ON u.id = p.user_id
             WHERE 1 = 1",
        );

        if let Some(user_id) = filter.user_id {
            query.push(" AND p.user_id = ").push_bind(user_id);
        }
        if let Some(document) = filter.document {
            query.push(" AND p.document = ").push_bind(document);
        }
        if let Some(device) = filter.device {
            query.push(" AND p.device = ").push_bind(device);
        }
        if let Some(device_id) = filter.device_id {
            query.push(" AND p.device_id = ").push_bind(device_id);
        }
        if let Some(from_ns) = filter.from_ns {
            query.push(" AND p.timestamp_ns >= ").push_bind(from_ns);
        }
        if let Some(to_ns) = filter.to_ns {
            query.push(" AND p.timestamp_ns < ").push_bind(to_ns);
        }
        if let Some(before_id) = before_id {
            query.push(" AND p.id < ").push_bind(before_id);
        }

        query.push(" ORDER BY p.id DESC LIMIT ").push_bind(limit);

        let entries = query.build_query_as().fetch_all(&db.read).await?;
        Ok(entries)
    }
}
