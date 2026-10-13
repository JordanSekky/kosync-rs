use sqlx::{FromRow, QueryBuilder, Sqlite};

use crate::{db::Db, error::Result};

/// A (name, id) pair a history entry was pushed from. The id is optional in
/// the protocol, and a device can be renamed, so one id can appear under
/// several names.
#[derive(Debug, Clone, FromRow)]
pub struct Device {
    pub device: String,
    pub device_id: Option<String>,
    pub entry_count: i64,
    /// Unix nanoseconds
    pub last_seen_ns: i64,
}

impl Device {
    /// Every device, most recently seen first, optionally only those one
    /// user has pushed from.
    pub async fn list(db: &Db, user_id: Option<i64>) -> Result<Vec<Self>> {
        let mut query = QueryBuilder::<Sqlite>::new(
            "SELECT device, device_id,
                    COUNT(*) AS entry_count,
                    MAX(timestamp_ns) AS last_seen_ns
             FROM progress_history",
        );
        if let Some(user_id) = user_id {
            query.push(" WHERE user_id = ").push_bind(user_id);
        }
        query.push(
            " GROUP BY device, device_id
             ORDER BY last_seen_ns DESC, device, device_id",
        );

        let devices = query.build_query_as().fetch_all(&db.read).await?;
        Ok(devices)
    }
}
