use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::routing::any;
use axum::{extract::Request, Router};
use http::StatusCode;

use super::super::{
    build_router, build_router_with_state, hash_api_key, sample_currently_usable_auth_snapshot,
    start_server, AppState, InMemoryAuthApiKeySnapshotRepository,
};
use crate::constants::{
    CONTROL_ENDPOINT_SIGNATURE_HEADER, CONTROL_EXECUTION_RUNTIME_HEADER,
    CONTROL_ROUTE_CLASS_HEADER, CONTROL_ROUTE_FAMILY_HEADER, CONTROL_ROUTE_KIND_HEADER,
    GATEWAY_HEADER, TRACE_ID_HEADER, TRUSTED_ADMIN_SESSION_ID_HEADER, TRUSTED_ADMIN_USER_ID_HEADER,
    TRUSTED_ADMIN_USER_ROLE_HEADER,
};

#[tokio::test]
async fn gateway_rejects_spoofed_admin_principal_headers_without_gateway_marker_locally() {
    let upstream_hits = Arc::new(Mutex::new(0usize));
    let upstream_hits_clone = Arc::clone(&upstream_hits);
    let upstream = Router::new().route(
        "/api/admin/endpoints/health/api-formats",
        any(move |_request: Request| {
            let upstream_hits_inner = Arc::clone(&upstream_hits_clone);
            async move {
                *upstream_hits_inner.lock().expect("mutex should lock") += 1;
                (StatusCode::OK, Body::from("unexpected upstream hit"))
            }
        }),
    );

    let (upstream_url, upstream_handle) = start_server(upstream).await;
    let gateway = build_router().expect("gateway should build");
    let (gateway_url, gateway_handle) = start_server(gateway).await;

    let response = reqwest::Client::new()
        .get(format!(
            "{gateway_url}/api/admin/endpoints/health/api-formats"
        ))
        .header(TRUSTED_ADMIN_USER_ID_HEADER, "admin-user-123")
        .header(TRUSTED_ADMIN_USER_ROLE_HEADER, "admin")
        .header(TRUSTED_ADMIN_SESSION_ID_HEADER, "session-123")
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let payload: serde_json::Value = response.json().await.expect("body should parse");
    assert_eq!(payload["detail"], "admin authentication required");
    assert_eq!(*upstream_hits.lock().expect("mutex should lock"), 0);

    gateway_handle.abort();
    upstream_handle.abort();
}

#[tokio::test]
async fn gateway_handles_models_support_routes_locally_with_public_support_control_headers() {
    let upstream_hits = Arc::new(Mutex::new(0usize));
    let upstream_hits_clone = Arc::clone(&upstream_hits);
    let upstream = Router::new().route(
        "/v1/models",
        any(move |_request: Request| {
            let upstream_hits_inner = Arc::clone(&upstream_hits_clone);
            async move {
                *upstream_hits_inner.lock().expect("mutex should lock") += 1;
                (
                    StatusCode::OK,
                    [(GATEWAY_HEADER, "fallback-probe")],
                    Body::from("{\"object\":\"list\",\"data\":[]}"),
                )
            }
        }),
    );

    let auth_repository = Arc::new(InMemoryAuthApiKeySnapshotRepository::seed(vec![(
        Some(hash_api_key("sk-test-models")),
        sample_currently_usable_auth_snapshot("key-models-1", "user-models-1"),
    )]));

    let (upstream_url, upstream_handle) = start_server(upstream).await;
    let gateway = build_router_with_state(
        AppState::new()
            .expect("gateway should build")
            .with_auth_api_key_data_reader_for_tests(auth_repository),
    );
    let (gateway_url, gateway_handle) = start_server(gateway).await;

    let response = reqwest::Client::new()
        .get(format!("{gateway_url}/v1/models?limit=20"))
        .header(http::header::AUTHORIZATION, "Bearer sk-test-models")
        .header(TRACE_ID_HEADER, "trace-models-123")
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        response
            .headers()
            .get(CONTROL_ROUTE_CLASS_HEADER)
            .and_then(|value| value.to_str().ok()),
        Some("public_support")
    );
    assert_eq!(
        response
            .headers()
            .get(CONTROL_EXECUTION_RUNTIME_HEADER)
            .and_then(|value| value.to_str().ok()),
        Some("false")
    );
    assert_eq!(
        response
            .headers()
            .get(CONTROL_ROUTE_FAMILY_HEADER)
            .and_then(|value| value.to_str().ok()),
        Some("models")
    );
    assert_eq!(
        response
            .headers()
            .get(CONTROL_ROUTE_KIND_HEADER)
            .and_then(|value| value.to_str().ok()),
        Some("list")
    );
    assert_eq!(
        response
            .headers()
            .get(CONTROL_ENDPOINT_SIGNATURE_HEADER)
            .and_then(|value| value.to_str().ok()),
        Some("openai:chat")
    );
    assert_eq!(
        response
            .headers()
            .get(TRACE_ID_HEADER)
            .and_then(|value| value.to_str().ok()),
        Some("trace-models-123")
    );
    let payload: serde_json::Value = response.json().await.expect("body should parse");
    assert_eq!(payload["detail"], "Route not found");
    assert_eq!(payload["route_family"], "models");
    assert_eq!(payload["route_kind"], "list");
    assert_eq!(*upstream_hits.lock().expect("mutex should lock"), 0);

    gateway_handle.abort();
    upstream_handle.abort();
}
