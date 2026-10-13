use std::{str::FromStr, thread::available_parallelism, time::Duration};

use chrono::{DateTime, Utc};
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};

/// Current time in unix nanoseconds, the resolution all timestamps are stored at.
pub fn now_ns() -> i64 {
    chrono::Utc::now()
        .timestamp_nanos_opt()
        .expect("system clock is outside the range of i64 nanoseconds")
}

/// A stored unix-nanosecond timestamp as a [`DateTime`].
pub fn datetime_from_ns(ns: i64) -> DateTime<Utc> {
    DateTime::from_timestamp_nanos(ns)
}

/// A [`DateTime`] as unix nanoseconds. Instants outside of what an `i64` can
/// hold saturate, which keeps them ordered correctly against every stored
/// timestamp when used as a range bound.
pub fn datetime_to_ns(datetime: DateTime<Utc>) -> i64 {
    datetime.timestamp_nanos_opt().unwrap_or_else(|| {
        if datetime.timestamp() < 0 {
            i64::MIN
        } else {
            i64::MAX
        }
    })
}

/// SQLite allows only one writer at a time, so all writes go through a
/// single-connection pool and reads get a pool of their own.
#[derive(Debug, Clone)]
pub struct Db {
    pub read: SqlitePool,
    pub write: SqlitePool,
}

impl Db {
    pub async fn connect(url: &str) -> Result<Self, sqlx::Error> {
        let options = SqliteConnectOptions::from_str(url)?
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5));

        let write = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options.clone().create_if_missing(true))
            .await?;

        sqlx::migrate!().run(&write).await?;

        let cpu_threads = available_parallelism().map_or(1, |n| n.get() as u32);
        let read = SqlitePoolOptions::new()
            .max_connections(cpu_threads)
            .connect_with(options.read_only(true))
            .await?;

        Ok(Self { read, write })
    }

    /// Waits for in-flight queries, then closes both pools. The write pool
    /// goes last so its connection is the final one open, which lets SQLite
    /// checkpoint the WAL and remove its `-wal` / `-shm` files.
    pub async fn close(&self) {
        self.read.close().await;
        self.write.close().await;
    }
}
