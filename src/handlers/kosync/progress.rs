use axum::{
    Json,
    extract::State,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use tokio_util::task::TaskTracker;
use tracing::Instrument;

use crate::{
    auth::AuthHeaders,
    authenticated_user::AuthenticatedUser,
    db::Db,
    error::Result,
    extract::{AppJson, AppPath},
    models::{Progress, ProgressUpdate},
    upstream::Upstream,
};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Metadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    filename: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    authors: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct UpdateProgressRequest {
    document: String,
    /// Opaque position: an XPointer for reflowable documents, a page number
    /// for paged ones
    progress: String,
    /// In [0, 1]
    percentage: f64,
    device: String,
    /// The client always sends it, but the reference server doesn't require it
    #[serde(skip_serializing_if = "Option::is_none")]
    device_id: Option<String>,
    /// Only sent when the user opts in
    #[serde(skip_serializing_if = "Option::is_none")]
    metadata: Option<Metadata>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct UpdateProgressResponse {
    document: String,
    /// Unix seconds
    timestamp: i64,
}

#[tracing::instrument(skip_all, fields(user_id = user.id, document = %request.document))]
pub async fn push(
    State(db): State<Db>,
    State(upstream): State<Option<Upstream>>,
    State(tasks): State<TaskTracker>,
    AuthHeaders { username, key }: AuthHeaders,
    AuthenticatedUser(user): AuthenticatedUser,
    AppJson(request): AppJson<UpdateProgressRequest>,
) -> Result<Json<UpdateProgressResponse>> {
    let metadata = request.metadata.as_ref();
    let insert = Progress::record(
        &db,
        user.id,
        ProgressUpdate {
            document: &request.document,
            progress: &request.progress,
            percentage: request.percentage,
            device: &request.device,
            device_id: request.device_id.as_deref(),
            filename: metadata.and_then(|x| x.filename.as_deref()),
            title: metadata.and_then(|x| x.title.as_deref()),
            authors: metadata.and_then(|x| x.authors.as_deref()),
        },
    )
    .await?;

    let timestamp = insert.timestamp_secs();

    if let Some(upstream) = upstream {
        let auth = AuthHeaders { username, key };
        // The client doesn't wait on the upstream server, and a failure there
        // only means it falls behind.
        // Spawned tasks don't inherit the current span, so carry it over.
        tasks.spawn(
            async move {
                match upstream.forward_push(&auth, &request).await {
                    Ok(()) => tracing::info!("forwarded progress upstream"),
                    Err(error) => tracing::warn!(%error, "failed to forward progress upstream"),
                }
            }
            .in_current_span(),
        );
    }

    Ok(Json(UpdateProgressResponse {
        document: insert.document,
        timestamp,
    }))
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GetProgressResponse {
    username: String,
    document: String,
    progress: String,
    percentage: f64,
    device: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    device_id: Option<String>,
    /// Unix seconds
    timestamp: i64,
}

#[tracing::instrument(skip_all, fields(user_id = user.id, document = %document))]
pub async fn pull(
    State(db): State<Db>,
    AuthenticatedUser(user): AuthenticatedUser,
    AppPath(document): AppPath<String>,
) -> Result<Response> {
    let Some(latest) = Progress::latest(&db, user.id, &document).await? else {
        // The client reads a body without `percentage` as "no progress yet";
        // 200 {} is what the reference server sends.
        return Ok(Json(serde_json::json!({})).into_response());
    };

    let timestamp = latest.timestamp_secs();

    Ok(Json(GetProgressResponse {
        username: user.username,
        document,
        progress: latest.progress,
        percentage: latest.percentage,
        device: latest.device,
        device_id: latest.device_id,
        timestamp,
    })
    .into_response())
}
