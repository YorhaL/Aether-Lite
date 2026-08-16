use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Json;
use axum::Router;
use serde_json::json;

use crate::constants::READYZ_PATH;
use crate::AppState;

pub(crate) fn mount_core_routes(router: Router<AppState>) -> Router<AppState> {
    router
        .route(READYZ_PATH, get(readyz))
        .route("/_gateway/health", get(health))
}

pub(crate) async fn health(State(state): State<AppState>) -> impl IntoResponse {
    let request_concurrency = state.request_concurrency_snapshot().map(|snapshot| {
        json!({
            "limit": snapshot.limit,
            "in_flight": snapshot.in_flight,
            "available_permits": snapshot.available_permits,
            "high_watermark": snapshot.high_watermark,
            "rejected": snapshot.rejected,
        })
    });
    let distributed_request_concurrency = state
        .distributed_request_concurrency_snapshot()
        .await
        .ok()
        .flatten()
        .map(|snapshot| {
            json!({
                "limit": snapshot.limit,
                "in_flight": snapshot.in_flight,
                "available_permits": snapshot.available_permits,
                "high_watermark": snapshot.high_watermark,
                "rejected": snapshot.rejected,
            })
        });
    Json(json!({
        "status": "ok",
        "component": "aether-gateway",
        "control_api_enabled": true,
        "request_concurrency": request_concurrency,
        "distributed_request_concurrency": distributed_request_concurrency,
    }))
}

pub(crate) async fn readyz(State(_state): State<AppState>) -> impl IntoResponse {
    Json(json!({
        "status": "ready",
        "component": "aether-gateway",
    }))
}
