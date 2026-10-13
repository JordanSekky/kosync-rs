use std::{
    collections::{BTreeMap, VecDeque},
    fmt,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::Serialize;
use tokio::sync::broadcast;
use tracing::{
    Event, Level, Subscriber,
    field::{Field, Visit},
    span,
};
use tracing_subscriber::{Layer, layer::Context, registry::LookupSpan};

/// How many entries a subscriber can fall behind by before it starts to lose
/// them. Unrelated to how many lines are cached.
const LIVE_CHANNEL_CAPACITY: usize = 1024;

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

/// The most recent log entries, kept in memory, plus a feed of new ones.
///
/// Doubles as the [`tracing_subscriber::Layer`] that fills it, so it has to be
/// added to the subscriber for anything to show up.
#[derive(Clone)]
pub struct LogBuffer(Arc<Shared>);

struct Shared {
    /// How many entries are kept for new subscribers; 0 keeps none
    capacity: usize,
    /// Identifies this run of the server, see [`LogBuffer::event_id`]
    epoch: u64,
    state: Mutex<State>,
    live: broadcast::Sender<Arc<LogEntry>>,
}

struct State {
    lines: VecDeque<Arc<LogEntry>>,
    next_id: u64,
}

impl LogBuffer {
    /// Keeps up to `capacity` of the most recent entries. With 0 nothing is
    /// cached and subscribers only see what is logged after they subscribe.
    pub fn new(capacity: usize) -> Self {
        let (live, _) = broadcast::channel(LIVE_CHANNEL_CAPACITY);

        Self(Arc::new(Shared {
            capacity,
            epoch: Utc::now().timestamp_millis().unsigned_abs(),
            state: Mutex::new(State {
                lines: VecDeque::with_capacity(capacity.min(LIVE_CHANNEL_CAPACITY)),
                next_id: 1,
            }),
            live,
        }))
    }

    /// The cached entries, oldest first and only those after `after` if
    /// given, together with a receiver for everything logged from here on.
    ///
    /// Both are taken under one lock, so no entry is in both or in neither.
    pub fn subscribe(
        &self,
        after: Option<u64>,
    ) -> (Vec<Arc<LogEntry>>, broadcast::Receiver<Arc<LogEntry>>) {
        let state = self.lock();
        let backlog = state
            .lines
            .iter()
            .filter(|entry| after.is_none_or(|after| entry.id > after))
            .cloned()
            .collect();

        (backlog, self.0.live.subscribe())
    }

    /// The SSE event id for an entry.
    ///
    /// Entry ids start over when the server restarts, so the id also names
    /// the run it came from. Otherwise a browser reconnecting with the
    /// `Last-Event-ID` of the previous run would be skipped past the new
    /// run's first entries.
    pub fn event_id(&self, entry: &LogEntry) -> String {
        format!("{}-{}", self.0.epoch, entry.id)
    }

    /// The entry id inside a `Last-Event-ID` header, or `None` if it didn't
    /// come from this run of the server and so can't be resumed from.
    pub fn resume_after(&self, last_event_id: &str) -> Option<u64> {
        let (epoch, id) = last_event_id.split_once('-')?;
        if epoch.parse::<u64>().ok()? != self.0.epoch {
            return None;
        }
        id.parse().ok()
    }

    fn record(
        &self,
        level: LogLevel,
        target: &str,
        message: String,
        fields: BTreeMap<String, String>,
        spans: Vec<LogSpan>,
    ) {
        let mut state = self.lock();

        let entry = Arc::new(LogEntry {
            id: state.next_id,
            timestamp: Utc::now(),
            level,
            target: target.to_owned(),
            message,
            fields,
            spans,
        });
        state.next_id += 1;

        if self.0.capacity > 0 {
            if state.lines.len() == self.0.capacity {
                state.lines.pop_front();
            }
            state.lines.push_back(entry.clone());
        }

        // Sent while still holding the lock, so receivers get entries in id
        // order and `subscribe` can't land between the push and the send.
        // An error only means nobody is listening.
        let _ = self.0.live.send(entry);
    }

    /// Never panics: this runs inside every `tracing` call, and a panic
    /// elsewhere with the lock held mustn't take logging down with it.
    fn lock(&self) -> MutexGuard<'_, State> {
        self.0.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The fields of a span, kept in its extensions until an event inside it is
/// logged.
struct SpanFields(BTreeMap<String, String>);

impl<S> Layer<S> for LogBuffer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &span::Id, ctx: Context<'_, S>) {
        let mut visitor = FieldVisitor::default();
        attrs.record(&mut visitor);

        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(SpanFields(visitor.fields));
        }
    }

    /// Fields declared empty on a span and filled in later
    fn on_record(&self, id: &span::Id, values: &span::Record<'_>, ctx: Context<'_, S>) {
        let mut visitor = FieldVisitor::default();
        values.record(&mut visitor);

        if let Some(span) = ctx.span(id)
            && let Some(fields) = span.extensions_mut().get_mut::<SpanFields>()
        {
            fields.0.extend(visitor.fields);
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let mut visitor = FieldVisitor::default();
        event.record(&mut visitor);
        let message = visitor.fields.remove("message").unwrap_or_default();

        let spans = ctx
            .event_scope(event)
            .map(|scope| {
                scope
                    .from_root()
                    .map(|span| LogSpan {
                        name: span.name().to_owned(),
                        fields: span
                            .extensions()
                            .get::<SpanFields>()
                            .map(|fields| fields.0.clone())
                            .unwrap_or_default(),
                    })
                    .collect()
            })
            .unwrap_or_default();

        let metadata = event.metadata();
        self.record(
            metadata.level().into(),
            metadata.target(),
            message,
            visitor.fields,
            spans,
        );
    }
}

#[derive(Default)]
struct FieldVisitor {
    fields: BTreeMap<String, String>,
}

// The other `record_*` methods default to `record_debug`, which is what
// `%value` and `?value` fields also arrive through.
impl Visit for FieldVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.fields
            .insert(field.name().to_owned(), value.to_owned());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.fields
            .insert(field.name().to_owned(), format!("{value:?}"));
    }
}

#[cfg(test)]
mod tests {
    use tracing_subscriber::layer::SubscriberExt;

    use super::*;

    fn log(buffer: &LogBuffer, message: &str) {
        buffer.record(
            LogLevel::Info,
            "test",
            message.to_owned(),
            BTreeMap::new(),
            Vec::new(),
        );
    }

    fn messages(entries: &[Arc<LogEntry>]) -> Vec<&str> {
        entries.iter().map(|e| e.message.as_str()).collect()
    }

    #[test]
    fn keeps_only_the_most_recent_entries() {
        let buffer = LogBuffer::new(3);
        for i in 1..=5 {
            log(&buffer, &i.to_string());
        }

        let (backlog, _) = buffer.subscribe(None);
        assert_eq!(messages(&backlog), ["3", "4", "5"]);
        assert_eq!(backlog.iter().map(|e| e.id).collect::<Vec<_>>(), [3, 4, 5]);
    }

    #[test]
    fn zero_capacity_caches_nothing_but_still_streams() {
        let buffer = LogBuffer::new(0);
        let (_, mut live) = buffer.subscribe(None);

        log(&buffer, "a");
        log(&buffer, "b");

        let (backlog, _) = buffer.subscribe(None);
        assert!(backlog.is_empty());
        assert_eq!(live.try_recv().unwrap().message, "a");
        assert_eq!(live.try_recv().unwrap().message, "b");
    }

    #[test]
    fn subscribers_get_each_entry_exactly_once() {
        let buffer = LogBuffer::new(10);
        log(&buffer, "before");

        let (backlog, mut live) = buffer.subscribe(None);
        log(&buffer, "after");

        assert_eq!(messages(&backlog), ["before"]);
        assert_eq!(live.try_recv().unwrap().message, "after");
        assert!(live.try_recv().is_err());
    }

    #[test]
    fn resumes_after_the_given_id() {
        let buffer = LogBuffer::new(10);
        for i in 1..=4 {
            log(&buffer, &i.to_string());
        }

        let (backlog, _) = buffer.subscribe(Some(2));
        assert_eq!(messages(&backlog), ["3", "4"]);
    }

    #[test]
    fn event_ids_round_trip_only_within_one_run() {
        let buffer = LogBuffer::new(10);
        log(&buffer, "a");
        let (backlog, _) = buffer.subscribe(None);
        let id = buffer.event_id(&backlog[0]);

        assert_eq!(buffer.resume_after(&id), Some(backlog[0].id));
        assert_eq!(buffer.resume_after("1-1"), None, "from another run");
        assert_eq!(buffer.resume_after("garbage"), None);
        assert_eq!(buffer.resume_after(""), None);
    }

    #[test]
    fn layer_records_message_level_and_fields() {
        let buffer = LogBuffer::new(10);
        let subscriber = tracing_subscriber::registry().with(buffer.clone());

        tracing::subscriber::with_default(subscriber, || {
            tracing::warn!(user = "ada", attempts = 3, "Too many attempts");
        });

        let (backlog, _) = buffer.subscribe(None);
        let entry = &backlog[0];
        assert_eq!(entry.level, LogLevel::Warn);
        assert_eq!(entry.message, "Too many attempts");
        assert_eq!(entry.fields["user"], "ada");
        assert_eq!(entry.fields["attempts"], "3");
        assert!(entry.target.starts_with("kosync_proxy"));
        assert!(entry.spans.is_empty());
    }

    #[test]
    fn layer_records_the_spans_an_event_is_inside_of() {
        let buffer = LogBuffer::new(10);
        let subscriber = tracing_subscriber::registry().with(buffer.clone());

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!("outside");

            let outer = tracing::info_span!("outer", user_id = 7, document = "abc");
            let _outer = outer.enter();
            let inner = tracing::info_span!("inner", status = tracing::field::Empty);
            let _inner = inner.enter();
            inner.record("status", "done");
            tracing::info!(extra = true, "inside");
        });

        let (backlog, _) = buffer.subscribe(None);
        assert!(backlog[0].spans.is_empty());

        let spans = &backlog[1].spans;
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].name, "outer", "outermost first");
        assert_eq!(spans[0].fields["user_id"], "7");
        assert_eq!(spans[0].fields["document"], "abc");
        assert_eq!(spans[1].name, "inner");
        assert_eq!(spans[1].fields["status"], "done", "recorded after creation");
        assert_eq!(backlog[1].fields["extra"], "true");
        assert!(!backlog[1].fields.contains_key("user_id"));
    }
}
