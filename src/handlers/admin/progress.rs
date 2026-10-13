use axum::{Json, extract::State};
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    db::Db,
    error::Result,
    extract::AppQuery,
    models::admin::{ProgressEntry, ProgressFilter},
    time::{datetime_from_ns, datetime_to_ns},
};

const DEFAULT_LIMIT: u32 = 100;
const MAX_LIMIT: u32 = 500;

#[derive(Deserialize, JsonSchema, Debug)]
pub struct ListProgressQuery {
    /// Only entries of this user
    user_id: Option<i64>,
    /// Only entries of this document, as reported by `listBooks`
    document: Option<String>,
    /// Only entries pushed from a device with this name
    device: Option<String>,
    /// Only entries pushed from a device with this id
    device_id: Option<String>,
    /// Only entries at or after this time
    from: Option<DateTime<Utc>>,
    /// Only entries before this time
    to: Option<DateTime<Utc>>,
    /// The `next_cursor` of the previous page
    cursor: Option<i64>,
    /// Page size, 100 if absent and at most 500
    limit: Option<u32>,
}

#[derive(Serialize, JsonSchema, Debug)]
pub struct ProgressEntryResponse {
    id: i64,
    user_id: i64,
    username: String,
    /// The document's id, which the client derives from the file
    document: String,
    /// Null unless the client sent metadata for the document
    title: Option<String>,
    authors: Option<String>,
    filename: Option<String>,
    /// Opaque position: an XPointer for reflowable documents, a page number
    /// for paged ones
    progress: String,
    /// In [0, 1]
    percentage: f64,
    device: String,
    device_id: Option<String>,
    /// When the server received the push
    timestamp: DateTime<Utc>,
}

impl From<ProgressEntry> for ProgressEntryResponse {
    fn from(entry: ProgressEntry) -> Self {
        Self {
            id: entry.id,
            user_id: entry.user_id,
            username: entry.username,
            document: entry.document,
            title: entry.title,
            authors: entry.authors,
            filename: entry.filename,
            progress: entry.progress,
            percentage: entry.percentage,
            device: entry.device,
            device_id: entry.device_id,
            timestamp: datetime_from_ns(entry.timestamp_ns),
        }
    }
}

#[derive(Serialize, JsonSchema, Debug)]
pub struct ListProgressResponse {
    /// Newest first
    entries: Vec<ProgressEntryResponse>,
    /// Pass as `cursor` to get the next page; null on the last page
    next_cursor: Option<i64>,
}

#[tracing::instrument(skip_all)]
pub async fn list(
    State(db): State<Db>,
    AppQuery(query): AppQuery<ListProgressQuery>,
) -> Result<Json<ListProgressResponse>> {
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT) as usize;

    let filter = ProgressFilter {
        user_id: query.user_id,
        document: query.document.as_deref(),
        device: query.device.as_deref(),
        device_id: query.device_id.as_deref(),
        from_ns: query.from.map(datetime_to_ns),
        to_ns: query.to.map(datetime_to_ns),
    };

    // One more than the page, to know whether there is a next one
    let mut entries = ProgressEntry::list(&db, &filter, query.cursor, limit as i64 + 1).await?;

    let next_cursor = if entries.len() > limit {
        entries.truncate(limit);
        entries.last().map(|entry| entry.id)
    } else {
        None
    };

    Ok(Json(ListProgressResponse {
        entries: entries.into_iter().map(Into::into).collect(),
        next_cursor,
    }))
}
