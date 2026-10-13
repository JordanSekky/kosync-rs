use axum::Json;
use schemars::JsonSchema;
use serde::Serialize;

#[derive(Serialize, JsonSchema, Debug)]
pub struct HealthResponse {
    /// Always "ok"
    status: &'static str,
}

/// Liveness only: it answers as long as the server is accepting requests.
pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}
