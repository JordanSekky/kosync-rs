use axum::{
    Json,
    extract::State,
    response::{IntoResponse, Response},
};
use tokio_util::task::TaskTracker;
use tracing::Instrument;

use crate::{
    auth::{AuthHeaders, AuthenticatedUser},
    db::Db,
    error::Result,
    extract::{AppJson, AppPath},
    models::{Progress, ProgressUpdate},
    protocol::{GetProgressResponse, UpdateProgressRequest, UpdateProgressResponse},
    upstream::Upstream,
};

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
