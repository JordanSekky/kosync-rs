//! The application's own log lines, kept in memory so the admin API can
//! replay and stream them.
//!
//! [`LogBuffer`] is both the store and the [`tracing_subscriber::Layer`] that
//! fills it, so it has to be added to the subscriber for anything to show up.

mod buffer;
mod entry;
mod layer;

pub use buffer::LogBuffer;
pub use entry::{LogEntry, LogLevel, LogSpan};
