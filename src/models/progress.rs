use sqlx::FromRow;

use crate::{
    db::{Db, now_ns},
    error::Result,
};

#[derive(Debug, Clone, FromRow)]
pub struct Progress {
    pub id: i64,
    pub user_id: i64,
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

/// A push from a client. The timestamp is not part of it: it is always
/// stamped by the server.
#[derive(Debug, Clone, Copy)]
pub struct ProgressUpdate<'a> {
    pub document: &'a str,
    pub progress: &'a str,
    pub percentage: f64,
    pub device: &'a str,
    pub device_id: Option<&'a str>,
    pub filename: Option<&'a str>,
    pub title: Option<&'a str>,
    pub authors: Option<&'a str>,
}

impl Progress {
    /// The timestamp in whole unix seconds, as the sync protocol reports it.
    pub fn timestamp_secs(&self) -> i64 {
        self.timestamp_ns.div_euclid(1_000_000_000)
    }

    /// Appends an entry to the history. Metadata fields omitted from the
    /// update are carried forward from the document's previous entry.
    pub async fn record(db: &Db, user_id: i64, update: ProgressUpdate<'_>) -> Result<Self> {
        let progress = sqlx::query_as(
            "INSERT INTO progress_history
                 (user_id, document, progress, percentage, device, device_id,
                  filename, title, authors, timestamp_ns)
             SELECT ?1, ?2, ?3, ?4, ?5, ?6,
                    COALESCE(?7, prev.filename),
                    COALESCE(?8, prev.title),
                    COALESCE(?9, prev.authors),
                    ?10
             FROM (SELECT 1) LEFT JOIN latest_progress AS prev
                 ON prev.user_id = ?1 AND prev.document = ?2
             RETURNING id, user_id, document, progress, percentage, device, device_id,
                       filename, title, authors, timestamp_ns",
        )
        .bind(user_id)
        .bind(update.document)
        .bind(update.progress)
        .bind(update.percentage)
        .bind(update.device)
        .bind(update.device_id)
        .bind(update.filename)
        .bind(update.title)
        .bind(update.authors)
        .bind(now_ns())
        .fetch_one(&db.write)
        .await?;
        Ok(progress)
    }

    /// The most recent entry for the document, from any device.
    pub async fn latest(db: &Db, user_id: i64, document: &str) -> Result<Option<Self>> {
        let progress = sqlx::query_as(
            "SELECT id, user_id, document, progress, percentage, device, device_id,
                    filename, title, authors, timestamp_ns
             FROM latest_progress WHERE user_id = ? AND document = ?",
        )
        .bind(user_id)
        .bind(document)
        .fetch_optional(&db.read)
        .await?;
        Ok(progress)
    }

    /// The most recent entry for the document from each device, newest first.
    pub async fn latest_per_device(db: &Db, user_id: i64, document: &str) -> Result<Vec<Self>> {
        let progress = sqlx::query_as(
            "SELECT id, user_id, document, progress, percentage, device, device_id,
                    filename, title, authors, timestamp_ns
             FROM latest_device_progress
             WHERE user_id = ? AND document = ?
             ORDER BY id DESC",
        )
        .bind(user_id)
        .bind(document)
        .fetch_all(&db.read)
        .await?;
        Ok(progress)
    }
}
