use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use chrono::Utc;
use tokio::sync::broadcast;

use super::{LogEntry, LogLevel, LogSpan};

/// How many entries a subscriber can fall behind by before it starts to lose
/// them. Unrelated to how many lines are cached.
const LIVE_CHANNEL_CAPACITY: usize = 1024;

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

    pub(super) fn record(
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

#[cfg(test)]
mod tests {
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
}
