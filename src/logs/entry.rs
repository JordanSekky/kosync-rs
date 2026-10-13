use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::Serialize;
use tracing::Level;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl From<&Level> for LogLevel {
    fn from(level: &Level) -> Self {
        match *level {
            Level::TRACE => Self::Trace,
            Level::DEBUG => Self::Debug,
            Level::INFO => Self::Info,
            Level::WARN => Self::Warn,
            Level::ERROR => Self::Error,
        }
    }
}

/// One event the application logged.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LogEntry {
    /// Consecutive within one run of the server, starting at 1. A gap means
    /// the client fell behind and some entries were skipped.
    pub id: u64,
    /// When the event was logged
    pub timestamp: DateTime<Utc>,
    pub level: LogLevel,
    /// The module that logged it, such as `kosync_proxy::upstream`
    pub target: String,
    /// Empty if the event had no message
    pub message: String,
    /// The event's other fields, with values rendered as text
    pub fields: BTreeMap<String, String>,
    /// The spans the event was logged inside of, outermost first. Handlers
    /// run in a span whose fields name the user and document, for example.
    pub spans: Vec<LogSpan>,
}

/// A span an event was logged inside of.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LogSpan {
    /// For handlers, the name of the function
    pub name: String,
    /// The span's fields, including ones recorded after it was created, with
    /// values rendered as text
    pub fields: BTreeMap<String, String>,
}
