use super::super::usage::{
    hash_api_key, sample_local_openai_auth_snapshot, sample_local_openai_candidate_row,
    sample_local_openai_endpoint, sample_local_openai_key, sample_local_openai_provider,
};
use crate::tests::{
    any, build_router_with_state, build_state_with_execution_runtime_override, json, start_server,
    AppState, Arc, Body, FrontdoorCorsConfig, Mutex, Request, Router, StatusCode,
};
use aether_crypto::DEVELOPMENT_ENCRYPTION_KEY;
use aether_data::repository::auth::InMemoryAuthApiKeySnapshotRepository;
use aether_data::repository::candidate_selection::InMemoryMinimalCandidateSelectionReadRepository;
use aether_data::repository::candidates::InMemoryRequestCandidateRepository;
use aether_data::repository::provider_catalog::InMemoryProviderCatalogReadRepository;
use aether_data::repository::usage::InMemoryUsageReadRepository;
use axum::Json;

use crate::data::GatewayDataState;

#[tokio::test]
async fn gateway_handles_cors_preflight_without_proxying_upstream() {
    let upstream_hits = Arc::new(Mutex::new(0usize));
    let upstream_hits_clone = Arc::clone(&upstream_hits);
    let upstream = Router::new().route(
        "/{*path}",
        any(move |_request: Request| {
            let upstream_hits_inner = Arc::clone(&upstream_hits_clone);
            async move {
                *upstream_hits_inner.lock().expect("mutex should lock") += 1;
                (StatusCode::OK, Body::from("proxied"))
            }
        }),
    );

    let (upstream_url, upstream_handle) = start_server(upstream).await;
    let state = AppState::new()
        .expect("state should build")
        .with_frontdoor_cors_config(
            FrontdoorCorsConfig::new(vec!["http://localhost:3000".to_string()], true)
                .expect("cors config should build"),
        );
    let gateway = build_router_with_state(state);
    let (gateway_url, gateway_handle) = start_server(gateway).await;

    let response = reqwest::Client::new()
        .request(
            reqwest::Method::OPTIONS,
            format!("{gateway_url}/v1/chat/completions"),
        )
        .header("origin", "http://localhost:3000")
        .header("access-control-request-method", "POST")
        .header(
            "access-control-request-headers",
            "authorization,content-type",
        )
        .send()
        .await
        .expect("preflight should succeed");

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        response
            .headers()
            .get("access-control-allow-origin")
            .expect("allow origin header"),
        "http://localhost:3000"
    );
    assert_eq!(
        response
            .headers()
            .get("access-control-allow-credentials")
            .expect("allow credentials header"),
        "true"
    );
    assert_eq!(*upstream_hits.lock().expect("mutex should lock"), 0);

    gateway_handle.abort();
    upstream_handle.abort();
}

#[test]
fn gateway_adds_cors_headers_to_proxied_responses() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_stack_size(16 * 1024 * 1024)
        .enable_all()
        .build()
        .expect("runtime should build");

    runtime.block_on(gateway_adds_cors_headers_to_proxied_responses_inner());
}

async fn gateway_adds_cors_headers_to_proxied_responses_inner() {
    let execution_runtime_hits = Arc::new(Mutex::new(0usize));
    let execution_runtime_hits_clone = Arc::clone(&execution_runtime_hits);
    let execution_runtime = Router::new().route(
        "/v1/execute/sync",
        any(move |_request: Request| {
            let execution_runtime_hits = Arc::clone(&execution_runtime_hits_clone);
            async move {
                *execution_runtime_hits.lock().expect("mutex should lock") += 1;
                Json(json!({
                    "request_id": "trace-openai-cors-proxy-123",
                    "status_code": 200,
                    "headers": {
                        "content-type": "application/json"
                    },
                    "body": {
                        "json_body": {
                            "id": "chatcmpl-cors-proxy-123",
                            "object": "chat.completion",
                            "model": "gpt-5-upstream",
                            "choices": []
                        }
                    }
                }))
            }
        }),
    );

    let auth_repository = Arc::new(InMemoryAuthApiKeySnapshotRepository::seed(vec![(
        Some(hash_api_key("sk-client-openai-cors")),
        sample_local_openai_auth_snapshot("api-key-openai-cors-1", "user-openai-cors-1"),
    )]));
    let candidate_selection_repository =
        Arc::new(InMemoryMinimalCandidateSelectionReadRepository::seed(vec![
            sample_local_openai_candidate_row(),
        ]));
    let provider_catalog_repository = Arc::new(InMemoryProviderCatalogReadRepository::seed(
        vec![sample_local_openai_provider()],
        vec![sample_local_openai_endpoint()],
        vec![sample_local_openai_key()],
    ));
    let request_candidate_repository = Arc::new(InMemoryRequestCandidateRepository::default());
    let usage_repository = Arc::new(InMemoryUsageReadRepository::default());

    let (execution_runtime_url, execution_runtime_handle) = start_server(execution_runtime).await;
    let state = build_state_with_execution_runtime_override(execution_runtime_url)
        .with_frontdoor_cors_config(
            FrontdoorCorsConfig::new(vec!["http://localhost:3000".to_string()], true)
                .expect("cors config should build"),
        )
        .with_data_state_for_tests(
            GatewayDataState::with_auth_candidate_selection_provider_catalog_request_candidates_and_usage_for_tests(
                auth_repository,
                candidate_selection_repository,
                provider_catalog_repository,
                request_candidate_repository,
                usage_repository,
                DEVELOPMENT_ENCRYPTION_KEY,
            ),
        );
    let gateway = build_router_with_state(state);
    let (gateway_url, gateway_handle) = start_server(gateway).await;

    let response = reqwest::Client::new()
        .post(format!("{gateway_url}/v1/chat/completions"))
        .header("origin", "http://localhost:3000")
        .header(http::header::AUTHORIZATION, "Bearer sk-client-openai-cors")
        .header(http::header::CONTENT_TYPE, "application/json")
        .body("{\"model\":\"gpt-5\",\"messages\":[]}")
        .send()
        .await
        .expect("proxy request should succeed");

    assert_eq!(response.status(), StatusCode::OK);
    let response_headers = response.headers().clone();
    assert_eq!(
        response
            .json::<serde_json::Value>()
            .await
            .expect("body should parse")["id"],
        "chatcmpl-cors-proxy-123"
    );
    assert_eq!(
        response_headers
            .get("access-control-allow-origin")
            .expect("allow origin header"),
        "http://localhost:3000"
    );
    assert_eq!(
        response_headers
            .get("access-control-expose-headers")
            .expect("expose headers header"),
        "*"
    );
    assert_eq!(
        *execution_runtime_hits.lock().expect("mutex should lock"),
        1
    );

    gateway_handle.abort();
    execution_runtime_handle.abort();
}
