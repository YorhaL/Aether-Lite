use std::time::{SystemTime, UNIX_EPOCH};

use aether_contracts::ExecutionResult;
use aether_data_contracts::repository::video_tasks::{
    StoredVideoTask, VideoTaskQueryFilter, VideoTaskStatus,
};
use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use axum::response::Redirect;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

mod cancel;

use super::query::VideoTaskVideoSource;
use super::{
    read_video_task_detail, read_video_task_page, read_video_task_stats,
    read_video_task_video_source,
};
use crate::{AppState, GatewayError};

pub(crate) use self::cancel::{cancel_video_task_record, CancelVideoTaskError};

#[derive(Debug, Deserialize)]
pub(crate) struct ListVideoTasksQuery {
    pub(crate) status: Option<String>,
    pub(crate) user_id: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) client_api_format: Option<String>,
    pub(crate) page: Option<usize>,
    pub(crate) page_size: Option<usize>,
}

pub(crate) async fn list_video_tasks(
    State(state): State<AppState>,
    Query(query): Query<ListVideoTasksQuery>,
) -> Result<Json<super::query::VideoTaskPageResponse>, axum::response::Response> {
    let filter = parse_filter(&query)?;
    let response = read_video_task_page(
        &state,
        &filter,
        query.page.unwrap_or(1),
        query.page_size.unwrap_or(20),
    )
    .await
    .map_err(IntoResponse::into_response)?;
    Ok(Json(response))
}

pub(crate) async fn get_video_task_stats(
    State(state): State<AppState>,
    Query(query): Query<ListVideoTasksQuery>,
) -> Result<Json<super::query::VideoTaskStatsResponse>, axum::response::Response> {
    let filter = parse_filter(&query)?;
    let response = read_video_task_stats(&state, &filter, current_unix_secs())
        .await
        .map_err(IntoResponse::into_response)?;
    Ok(Json(response))
}

pub(crate) async fn get_video_task_detail(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
) -> Result<Json<StoredVideoTask>, axum::response::Response> {
    let task = read_video_task_detail(&state, &task_id)
        .await
        .map_err(IntoResponse::into_response)?;

    match task {
        Some(task) => Ok(Json(task)),
        None => Err((
            axum::http::StatusCode::NOT_FOUND,
            Json(json!({
                "error": {
                    "message": "Video task not found",
                }
            })),
        )
            .into_response()),
    }
}

pub(crate) async fn cancel_video_task(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
) -> Result<Json<Value>, axum::response::Response> {
    let stored = cancel_video_task_record(&state, &task_id)
        .await
        .map_err(|err| match err {
            CancelVideoTaskError::NotFound => (
                axum::http::StatusCode::NOT_FOUND,
                Json(json!({
                    "error": {
                        "message": "Video task not found",
                    }
                })),
            )
                .into_response(),
            CancelVideoTaskError::InvalidStatus(status) => (
                axum::http::StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": {
                        "message": format!(
                            "Cannot cancel task with status: {}",
                            video_task_status_name(status),
                        ),
                    }
                })),
            )
                .into_response(),
            CancelVideoTaskError::Response(response) => response,
            CancelVideoTaskError::Gateway(err) => err.into_response(),
        })?;

    Ok(Json(json!({
        "id": stored.id,
        "status": "cancelled",
        "message": "Task cancelled successfully",
    })))
}

pub(crate) async fn get_video_task_video(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
) -> Result<axum::response::Response, axum::response::Response> {
    let Some(source) = read_video_task_video_source(&state, &task_id)
        .await
        .map_err(IntoResponse::into_response)?
    else {
        return Err((
            axum::http::StatusCode::NOT_FOUND,
            Json(json!({
                "error": {
                    "message": "Video task or video not found",
                }
            })),
        )
            .into_response());
    };

    build_video_task_video_response(source)
        .await
        .map_err(IntoResponse::into_response)
}

pub(crate) async fn build_video_task_video_response(
    source: VideoTaskVideoSource,
) -> Result<axum::response::Response, GatewayError> {
    match source {
        VideoTaskVideoSource::Redirect { url } => Ok(Redirect::temporary(&url).into_response()),
    }
}

fn parse_filter(
    query: &ListVideoTasksQuery,
) -> Result<VideoTaskQueryFilter, axum::response::Response> {
    let status = match query.status.as_deref() {
        Some(value) => Some(VideoTaskStatus::from_database(value).map_err(|err| {
            (
                axum::http::StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": {
                        "message": err.to_string(),
                    }
                })),
            )
                .into_response()
        })?),
        None => None,
    };

    Ok(VideoTaskQueryFilter {
        user_id: query.user_id.clone(),
        status,
        model_substring: query.model.clone(),
        client_api_format: query.client_api_format.clone(),
    })
}

fn current_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn video_task_status_name(status: VideoTaskStatus) -> &'static str {
    match status {
        VideoTaskStatus::Pending => "pending",
        VideoTaskStatus::Submitted => "submitted",
        VideoTaskStatus::Queued => "queued",
        VideoTaskStatus::Processing => "processing",
        VideoTaskStatus::Completed => "completed",
        VideoTaskStatus::Failed => "failed",
        VideoTaskStatus::Cancelled => "cancelled",
        VideoTaskStatus::Expired => "expired",
        VideoTaskStatus::Deleted => "deleted",
    }
}
