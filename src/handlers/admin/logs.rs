use axum::{extract::State, response::sse::Event};
use futures_util::{StreamExt, stream};
use tokio::sync::broadcast::error::RecvError;
use tokio_util::sync::CancellationToken;

use super::sse::{LastEventId, LogStream};
use crate::logs::LogBuffer;

/// Sends the cached entries, then everything logged from then on.
///
/// Ends when the client goes away or the server starts shutting down, which
/// graceful shutdown would otherwise wait on for as long as a page is open.
pub async fn stream(
    State(buffer): State<LogBuffer>,
    State(shutdown): State<CancellationToken>,
    LastEventId(last_event_id): LastEventId,
) -> LogStream {
    let after = last_event_id.and_then(|id| buffer.resume_after(&id));
    let (backlog, receiver) = buffer.subscribe(after);

    // A receiver that falls too far behind skips ahead. The gap shows in the
    // entry ids, so it is skipped over here rather than ending the stream.
    let live = stream::unfold(receiver, |mut receiver| async move {
        loop {
            match receiver.recv().await {
                Ok(entry) => return Some((entry, receiver)),
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => return None,
            }
        }
    });

    let events = stream::iter(backlog)
        .chain(live)
        .take_until(shutdown.cancelled_owned())
        .map(move |entry| {
            Event::default()
                .id(buffer.event_id(&entry))
                .json_data(&*entry)
        });

    LogStream::new(events.boxed())
}
