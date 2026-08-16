use std::sync::{Arc, Mutex};

use aether_data::repository::provider_catalog::InMemoryProviderCatalogReadRepository;
use aether_data_contracts::repository::provider_catalog::{
    ProviderCatalogReadRepository, StoredProviderCatalogKey,
};
use axum::body::Body;
use axum::routing::any;
use axum::{extract::Request, Router};
use http::StatusCode;
use serde_json::json;

use super::super::{build_router_with_state, sample_key, sample_provider, start_server, AppState};
use crate::constants::{
    GATEWAY_HEADER, TRUSTED_ADMIN_SESSION_ID_HEADER, TRUSTED_ADMIN_USER_ID_HEADER,
    TRUSTED_ADMIN_USER_ROLE_HEADER,
};
use crate::data::GatewayDataState;

fn adaptive_repository_with_keys(
    keys: Vec<StoredProviderCatalogKey>,
) -> Arc<InMemoryProviderCatalogReadRepository> {
    Arc::new(InMemoryProviderCatalogReadRepository::seed(
        vec![sample_provider("provider-openai", "openai", 10)],
        vec![],
        keys,
    ))
}

async fn assert_adaptive_route_returns_local_503(
    data_state: crate::data::GatewayDataState,
    method: http::Method,
    path: &str,
    body: Option<serde_json::Value>,
    expected_message: &str,
) {
    let upstream_hits = Arc::new(Mutex::new(0usize));
    let upstream_hits_clone = Arc::clone(&upstream_hits);
    let upstream = Router::new().route(
        "/{*path}",
        any(move |_request: Request| {
            let upstream_hits_inner = Arc::clone(&upstream_hits_clone);
            async move {
                *upstream_hits_inner.lock().expect("mutex should lock") += 1;
                (StatusCode::OK, Body::from("unexpected upstream hit"))
            }
        }),
    );

    let (upstream_url, upstream_handle) = start_server(upstream).await;
    let gateway = build_router_with_state(
        AppState::new()
            .expect("gateway should build")
            .with_data_state_for_tests(data_state),
    );
    let (gateway_url, gateway_handle) = start_server(gateway).await;

    let mut request = reqwest::Client::new()
        .request(method, format!("{gateway_url}{path}"))
        .header(crate::constants::GATEWAY_HEADER, "aether")
        .header(TRUSTED_ADMIN_USER_ID_HEADER, "admin-user-123")
        .header(TRUSTED_ADMIN_USER_ROLE_HEADER, "admin")
        .header(TRUSTED_ADMIN_SESSION_ID_HEADER, "session-123");
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request.send().await.expect("request should succeed");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let payload: serde_json::Value = response.json().await.expect("json body should parse");
    assert_eq!(payload["error"]["message"], expected_message);
    assert_eq!(*upstream_hits.lock().expect("mutex should lock"), 0);

    gateway_handle.abort();
    upstream_handle.abort();
}

const ADAPTIVE_DATA_UNAVAILABLE_MESSAGE: &str = "Admin adaptive data unavailable";

#[tokio::test]
async fn gateway_handles_admin_adaptive_keys_locally_with_trusted_admin_principal() {
    let upstream_hits = Arc::new(Mutex::new(0usize));
    let upstream_hits_clone = Arc::clone(&upstream_hits);
    let upstream = Router::new().route(
        "/{*path}",
        any(move |_request: Request| {
            let upstream_hits_inner = Arc::clone(&upstream_hits_clone);
            async move {
                *upstream_hits_inner.lock().expect("mutex should lock") += 1;
                (StatusCode::OK, Body::from("unexpected upstream hit"))
            }
        }),
    );

    let mut adaptive_key = sample_key("key-adaptive", "provider-openai", "openai:chat", "sk-test");
    adaptive_key.rpm_limit = None;
    adaptive_key.learned_rpm_limit = Some(17);
    adaptive_key.concurrent_429_count = Some(2);
    adaptive_key.rpm_429_count = Some(1);

    let mut fixed_key = sample_key("key-fixed", "provider-openai", "openai:chat", "sk-fixed");
    fixed_key.rpm_limit = Some(9);
    fixed_key.learned_rpm_limit = Some(11);

    let repository = adaptive_repository_with_keys(vec![adaptive_key, fixed_key]);

    let (upstream_url, upstream_handle) = start_server(upstream).await;
    let gateway = build_router_with_state(
        AppState::new()
            .expect("gateway should build")
            .with_data_state_for_tests(
                GatewayDataState::with_provider_catalog_repository_for_tests(repository),
            ),
    );
    let (gateway_url, gateway_handle) = start_server(gateway).await;

    let response = reqwest::Client::new()
        .get(format!("{gateway_url}/api/admin/adaptive/keys"))
        .header(crate::constants::GATEWAY_HEADER, "aether")
        .header(TRUSTED_ADMIN_USER_ID_HEADER, "admin-user-123")
        .header(TRUSTED_ADMIN_USER_ROLE_HEADER, "admin")
        .header(TRUSTED_ADMIN_SESSION_ID_HEADER, "session-123")
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::OK);
    let payload: serde_json::Value = response.json().await.expect("json body should parse");
    let items = payload.as_array().expect("items should be an array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], "key-adaptive");
    assert_eq!(items[0]["is_adaptive"], true);
    assert_eq!(items[0]["effective_limit"], 17);
    assert_eq!(*upstream_hits.lock().expect("mutex should lock"), 0);

    gateway_handle.abort();
    upstream_handle.abort();
}

#[tokio::test]
async fn gateway_handles_admin_adaptive_summary_locally_with_trusted_admin_principal() {
    let upstream_hits = Arc::new(Mutex::new(0usize));
    let upstream_hits_clone = Arc::clone(&upstream_hits);
    let upstream = Router::new().route(
        "/{*path}",
        any(move |_request: Request| {
            let upstream_hits_inner = Arc::clone(&upstream_hits_clone);
            async move {
                *upstream_hits_inner.lock().expect("mutex should lock") += 1;
                (StatusCode::OK, Body::from("unexpected upstream hit"))
            }
        }),
    );

    let mut first_key = sample_key("key-a", "provider-openai", "openai:chat", "sk-a");
    first_key.rpm_limit = None;
    first_key.concurrent_429_count = Some(2);
    first_key.rpm_429_count = Some(3);
    first_key.adjustment_history = Some(json!([
        {"timestamp": "2026-03-28T15:00:00Z", "delta": 2},
        {"timestamp": "2026-03-28T14:00:00Z", "delta": -1}
    ]));

    let mut second_key = sample_key("key-b", "provider-openai", "openai:chat", "sk-b");
    second_key.rpm_limit = None;
    second_key.concurrent_429_count = Some(1);
    second_key.rpm_429_count = Some(1);
    second_key.adjustment_history = Some(json!([
        {"timestamp": "2026-03-28T16:00:00Z", "delta": 1}
    ]));

    let repository = adaptive_repository_with_keys(vec![first_key, second_key]);

    let (upstream_url, upstream_handle) = start_server(upstream).await;
    let gateway = build_router_with_state(
        AppState::new()
            .expect("gateway should build")
            .with_data_state_for_tests(
                GatewayDataState::with_provider_catalog_repository_for_tests(repository),
            ),
    );
    let (gateway_url, gateway_handle) = start_server(gateway).await;

    let response = reqwest::Client::new()
        .get(format!("{gateway_url}/api/admin/adaptive/summary"))
        .header(crate::constants::GATEWAY_HEADER, "aether")
        .header(TRUSTED_ADMIN_USER_ID_HEADER, "admin-user-123")
        .header(TRUSTED_ADMIN_USER_ROLE_HEADER, "admin")
        .header(TRUSTED_ADMIN_SESSION_ID_HEADER, "session-123")
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::OK);
    let payload: serde_json::Value = response.json().await.expect("json body should parse");
    assert_eq!(payload["total_adaptive_keys"], 2);
    assert_eq!(payload["total_concurrent_429_errors"], 3);
    assert_eq!(payload["total_rpm_429_errors"], 4);
    assert_eq!(payload["total_adjustments"], 3);
    assert_eq!(payload["recent_adjustments"][0]["key_id"], "key-b");
    assert_eq!(*upstream_hits.lock().expect("mutex should lock"), 0);

    gateway_handle.abort();
    upstream_handle.abort();
}

#[tokio::test]
async fn gateway_handles_admin_adaptive_keys_returns_service_unavailable_when_disabled() {
    assert_adaptive_route_returns_local_503(
        crate::data::GatewayDataState::disabled(),
        http::Method::GET,
        "/api/admin/adaptive/keys",
        None,
        ADAPTIVE_DATA_UNAVAILABLE_MESSAGE,
    )
    .await;
}

#[tokio::test]
async fn gateway_handles_admin_adaptive_mode_returns_service_unavailable_without_writer() {
    let mut adaptive_key = sample_key("key-adaptive", "provider-openai", "openai:chat", "sk-test");
    adaptive_key.rpm_limit = None;
    let repository = adaptive_repository_with_keys(vec![adaptive_key]);

    assert_adaptive_route_returns_local_503(
        crate::data::GatewayDataState::with_provider_catalog_reader_for_tests(repository),
        http::Method::PATCH,
        "/api/admin/adaptive/keys/key-adaptive/mode",
        Some(json!({ "enabled": false, "fixed_limit": 20 })),
        ADAPTIVE_DATA_UNAVAILABLE_MESSAGE,
    )
    .await;
}

#[tokio::test]
async fn gateway_handles_admin_adaptive_stats_locally_with_trusted_admin_principal() {
    let upstream_hits = Arc::new(Mutex::new(0usize));
    let upstream_hits_clone = Arc::clone(&upstream_hits);
    let upstream = Router::new().route(
        "/{*path}",
        any(move |_request: Request| {
            let upstream_hits_inner = Arc::clone(&upstream_hits_clone);
            async move {
                *upstream_hits_inner.lock().expect("mutex should lock") += 1;
                (StatusCode::OK, Body::from("unexpected upstream hit"))
            }
        }),
    );

    let mut key = sample_key("key-openai", "provider-openai", "openai:chat", "sk-test");
    key.rpm_limit = None;
    key.learned_rpm_limit = Some(12);
    key.concurrent_429_count = Some(2);
    key.rpm_429_count = Some(1);
    key.last_429_at_unix_secs = Some(1_711_111_111);
    key.last_429_type = Some("rpm".to_string());
    key.adjustment_history = Some(json!([
        {"timestamp": "2026-03-28T14:00:00Z", "delta": 2}
    ]));
    key.status_snapshot = Some(json!({
        "learning_confidence": 0.9,
        "enforcement_active": true,
        "observation_count": 8,
        "header_observation_count": 3,
        "latest_upstream_limit": 15
    }));
    let repository = adaptive_repository_with_keys(vec![key]);

    let (upstream_url, upstream_handle) = start_server(upstream).await;
    let gateway = build_router_with_state(
        AppState::new()
            .expect("gateway should build")
            .with_data_state_for_tests(
                GatewayDataState::with_provider_catalog_repository_for_tests(repository),
            ),
    );
    let (gateway_url, gateway_handle) = start_server(gateway).await;

    let response = reqwest::Client::new()
        .get(format!(
            "{gateway_url}/api/admin/adaptive/keys/key-openai/stats"
        ))
        .header(crate::constants::GATEWAY_HEADER, "aether")
        .header(TRUSTED_ADMIN_USER_ID_HEADER, "admin-user-123")
        .header(TRUSTED_ADMIN_USER_ROLE_HEADER, "admin")
        .header(TRUSTED_ADMIN_SESSION_ID_HEADER, "session-123")
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::OK);
    let payload: serde_json::Value = response.json().await.expect("json body should parse");
    assert_eq!(payload["adaptive_mode"], true);
    assert_eq!(payload["effective_limit"], 12);
    assert_eq!(payload["learning_confidence"], 0.9);
    assert_eq!(payload["latest_upstream_limit"], 15);
    assert_eq!(*upstream_hits.lock().expect("mutex should lock"), 0);

    gateway_handle.abort();
    upstream_handle.abort();
}

#[tokio::test]
async fn gateway_toggles_admin_adaptive_mode_locally_with_trusted_admin_principal() {
    let repository = adaptive_repository_with_keys({
        let mut key = sample_key("key-openai", "provider-openai", "openai:chat", "sk-test");
        key.rpm_limit = None;
        key.learned_rpm_limit = Some(18);
        vec![key]
    });

    let gateway = build_router_with_state(
        AppState::new()
            .expect("gateway should build")
            .with_data_state_for_tests(
                GatewayDataState::with_provider_catalog_repository_for_tests(Arc::clone(
                    &repository,
                )),
            ),
    );
    let (gateway_url, gateway_handle) = start_server(gateway).await;

    let response = reqwest::Client::new()
        .patch(format!(
            "{gateway_url}/api/admin/adaptive/keys/key-openai/mode"
        ))
        .header(crate::constants::GATEWAY_HEADER, "aether")
        .header(TRUSTED_ADMIN_USER_ID_HEADER, "admin-user-123")
        .header(TRUSTED_ADMIN_USER_ROLE_HEADER, "admin")
        .header(TRUSTED_ADMIN_SESSION_ID_HEADER, "session-123")
        .json(&json!({ "enabled": false, "fixed_limit": 12 }))
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::OK);
    let payload: serde_json::Value = response.json().await.expect("json body should parse");
    assert_eq!(payload["is_adaptive"], false);
    assert_eq!(payload["rpm_limit"], 12);

    let updated = repository
        .list_keys_by_ids(&["key-openai".to_string()])
        .await
        .expect("repository query should succeed");
    assert_eq!(updated[0].rpm_limit, Some(12));

    gateway_handle.abort();
}

#[tokio::test]
async fn gateway_sets_admin_adaptive_limit_locally_with_trusted_admin_principal() {
    let repository = adaptive_repository_with_keys({
        let mut key = sample_key("key-openai", "provider-openai", "openai:chat", "sk-test");
        key.rpm_limit = None;
        vec![key]
    });

    let gateway = build_router_with_state(
        AppState::new()
            .expect("gateway should build")
            .with_data_state_for_tests(
                GatewayDataState::with_provider_catalog_repository_for_tests(Arc::clone(
                    &repository,
                )),
            ),
    );
    let (gateway_url, gateway_handle) = start_server(gateway).await;

    let response = reqwest::Client::new()
        .patch(format!(
            "{gateway_url}/api/admin/adaptive/keys/key-openai/limit?limit=9"
        ))
        .header(crate::constants::GATEWAY_HEADER, "aether")
        .header(TRUSTED_ADMIN_USER_ID_HEADER, "admin-user-123")
        .header(TRUSTED_ADMIN_USER_ROLE_HEADER, "admin")
        .header(TRUSTED_ADMIN_SESSION_ID_HEADER, "session-123")
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::OK);
    let payload: serde_json::Value = response.json().await.expect("json body should parse");
    assert_eq!(payload["rpm_limit"], 9);
    assert_eq!(payload["is_adaptive"], false);

    let updated = repository
        .list_keys_by_ids(&["key-openai".to_string()])
        .await
        .expect("repository query should succeed");
    assert_eq!(updated[0].rpm_limit, Some(9));

    gateway_handle.abort();
}

#[tokio::test]
async fn gateway_resets_admin_adaptive_learning_locally_with_trusted_admin_principal() {
    let repository = adaptive_repository_with_keys({
        let mut key = sample_key("key-openai", "provider-openai", "openai:chat", "sk-test");
        key.rpm_limit = None;
        key.learned_rpm_limit = Some(18);
        key.concurrent_429_count = Some(2);
        key.rpm_429_count = Some(1);
        key.last_429_at_unix_secs = Some(1_711_111_111);
        key.last_429_type = Some("rpm".to_string());
        key.adjustment_history = Some(json!([
            {"timestamp": "2026-03-28T14:00:00Z", "delta": 2}
        ]));
        vec![key]
    });

    let gateway = build_router_with_state(
        AppState::new()
            .expect("gateway should build")
            .with_data_state_for_tests(
                GatewayDataState::with_provider_catalog_repository_for_tests(Arc::clone(
                    &repository,
                )),
            ),
    );
    let (gateway_url, gateway_handle) = start_server(gateway).await;

    let response = reqwest::Client::new()
        .delete(format!(
            "{gateway_url}/api/admin/adaptive/keys/key-openai/learning"
        ))
        .header(crate::constants::GATEWAY_HEADER, "aether")
        .header(TRUSTED_ADMIN_USER_ID_HEADER, "admin-user-123")
        .header(TRUSTED_ADMIN_USER_ROLE_HEADER, "admin")
        .header(TRUSTED_ADMIN_SESSION_ID_HEADER, "session-123")
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::OK);
    let payload: serde_json::Value = response.json().await.expect("json body should parse");
    assert_eq!(payload["message"], "学习状态已重置");

    let updated = repository
        .list_keys_by_ids(&["key-openai".to_string()])
        .await
        .expect("repository query should succeed");
    assert_eq!(updated[0].learned_rpm_limit, None);
    assert_eq!(updated[0].concurrent_429_count, Some(0));
    assert_eq!(updated[0].rpm_429_count, Some(0));
    assert_eq!(updated[0].adjustment_history, None);

    gateway_handle.abort();
}
