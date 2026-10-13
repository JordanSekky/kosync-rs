use std::{collections::BTreeMap, fmt};

use tracing::{
    Event, Subscriber,
    field::{Field, Visit},
    span,
};
use tracing_subscriber::{Layer, layer::Context, registry::LookupSpan};

use super::{LogBuffer, LogSpan};

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
    use crate::logs::LogLevel;

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
