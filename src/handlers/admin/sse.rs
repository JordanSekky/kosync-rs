//! Server-sent events in the admin API, with the pieces aide has no notion of
//! described for the OpenAPI document.

use std::convert::Infallible;

use aide::{
    OperationInput, OperationOutput,
    generate::GenContext,
    openapi::{
        HeaderStyle, MediaType, Operation, Parameter, ParameterData, ParameterSchemaOrContent,
        ReferenceOr, Response, SchemaObject,
    },
};
use axum::{
    extract::FromRequestParts,
    http::{HeaderName, HeaderValue, request::Parts},
    response::{
        IntoResponse,
        sse::{Event, KeepAlive, KeepAliveStream, Sse},
    },
};
use futures_util::stream::BoxStream;

use crate::logs::LogEntry;

/// The `Last-Event-ID` request header a browser's `EventSource` sends when it
/// reconnects, naming the last event it received.
pub struct LastEventId(pub Option<String>);

impl<S: Send + Sync> FromRequestParts<S> for LastEventId {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        let id = parts
            .headers
            .get("last-event-id")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);

        Ok(Self(id))
    }
}

impl OperationInput for LastEventId {
    fn operation_input(ctx: &mut GenContext, operation: &mut Operation) {
        let json_schema = ctx.schema.subschema_for::<String>();

        operation
            .parameters
            .push(ReferenceOr::Item(Parameter::Header {
                parameter_data: ParameterData {
                    name: "Last-Event-ID".to_owned(),
                    description: Some(
                        "The `id` of the last event received, which `EventSource` sends by \
                         itself when it reconnects. The stream then continues after that entry \
                         instead of replaying the cache. Ignored if it came from before the \
                         server last restarted."
                            .to_owned(),
                    ),
                    required: false,
                    deprecated: None,
                    format: ParameterSchemaOrContent::Schema(SchemaObject {
                        json_schema,
                        example: None,
                        external_docs: None,
                    }),
                    example: None,
                    examples: Default::default(),
                    explode: None,
                    extensions: Default::default(),
                },
                style: HeaderStyle::Simple,
            }));
    }
}

/// A server-sent event stream of [`LogEntry`]s.
///
/// aide has no notion of [`Sse`], so this stands in for it in the spec: a
/// `text/event-stream` response whose events carry a `LogEntry` as JSON.
pub struct LogStream(Sse<KeepAliveStream<BoxStream<'static, Result<Event, axum::Error>>>>);

impl LogStream {
    /// Sends `events` as they come, with keep-alive comments in between so
    /// that idle connections aren't dropped by proxies.
    pub fn new(events: BoxStream<'static, Result<Event, axum::Error>>) -> Self {
        Self(Sse::new(events).keep_alive(KeepAlive::default()))
    }
}

impl IntoResponse for LogStream {
    fn into_response(self) -> axum::response::Response {
        let mut response = self.0.into_response();
        // Tells nginx not to hold the stream back to fill a buffer
        response.headers_mut().insert(
            HeaderName::from_static("x-accel-buffering"),
            HeaderValue::from_static("no"),
        );
        response
    }
}

impl OperationOutput for LogStream {
    type Inner = LogEntry;

    fn operation_response(ctx: &mut GenContext, _operation: &mut Operation) -> Option<Response> {
        let json_schema = ctx.schema.subschema_for::<LogEntry>();

        Some(Response {
            description: "A `text/event-stream` that stays open until the client disconnects or \
                          the server shuts down. Each event's `data` is one `LogEntry` as JSON, \
                          and its `id` can be sent back as `Last-Event-ID`."
                .to_owned(),
            content: [(
                "text/event-stream".to_owned(),
                MediaType {
                    schema: Some(SchemaObject {
                        json_schema,
                        example: None,
                        external_docs: None,
                    }),
                    ..Default::default()
                },
            )]
            .into_iter()
            .collect(),
            ..Default::default()
        })
    }

    fn inferred_responses(
        ctx: &mut GenContext,
        operation: &mut Operation,
    ) -> Vec<(Option<u16>, Response)> {
        Self::operation_response(ctx, operation)
            .map(|response| vec![(Some(200), response)])
            .unwrap_or_default()
    }
}
