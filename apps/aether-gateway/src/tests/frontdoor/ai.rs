use super::{
    hash_api_key, sample_models_candidate_row, unrestricted_models_snapshot,
    InMemoryAuthApiKeySnapshotRepository, InMemoryMinimalCandidateSelectionReadRepository,
    StoredAuthApiKeySnapshot, DEVELOPMENT_ENCRYPTION_KEY,
};
use crate::image_capabilities::openai_image_gateway_max_generation_count;
use crate::tests::{
    any, build_router_with_state, json, start_server, AppState, Arc, Body, Json, Mutex, Request,
    Router, StatusCode, EXECUTION_PATH_HEADER,
    EXECUTION_PATH_LOCAL_AI_PUBLIC, EXECUTION_PATH_LOCAL_EXECUTION_RUNTIME_MISS,
};
use aether_data::repository::global_models::InMemoryGlobalModelReadRepository;
use aether_data::DataLayerError;
use aether_data_contracts::repository::candidate_selection::{
    MinimalCandidateSelectionReadRepository, StoredMinimalCandidateSelectionRow,
    StoredRequestedModelCandidateRowsQuery,
};
use aether_data_contracts::repository::global_models::{
    StoredAdminGlobalModel, UpdateAdminGlobalModelRecord,
};
use async_trait::async_trait;
use axum::response::IntoResponse;
use std::collections::HashMap;
use std::future::pending;
use std::sync::atomic::{AtomicBool, Ordering};

fn codex_models_snapshot(api_key_id: &str, user_id: &str) -> StoredAuthApiKeySnapshot {
    StoredAuthApiKeySnapshot::new(
        user_id.to_string(),
        "alice".to_string(),
        Some("alice@example.com".to_string()),
        "user".to_string(),
        "local".to_string(),
        true,
        false,
        Some(json!(["codex"])),
        Some(json!(["openai:responses"])),
        Some(json!(["frontier-sol", "broken-luna"])),
        api_key_id.to_string(),
        Some("codex-models".to_string()),
        true,
        false,
        false,
        Some(10),
        Some(5),
        Some(4_102_444_800),
        Some(json!(["codex"])),
        Some(json!(["openai:responses"])),
        Some(json!(["frontier-sol", "broken-luna"])),
    )
    .expect("Codex models auth snapshot should build")
}

fn sample_codex_models_candidate_row(
    provider_id: &str,
    global_model_name: &str,
    source_model_name: &str,
) -> StoredMinimalCandidateSelectionRow {
    let mut row = sample_models_candidate_row(
        provider_id,
        "codex",
        "openai:responses",
        global_model_name,
        10,
    );
    row.model_provider_model_name = source_model_name.to_string();
    row.model_provider_model_mappings = Some(vec![
        aether_data_contracts::repository::candidate_selection::StoredProviderModelMapping {
            name: source_model_name.to_string(),
            priority: 1,
            api_formats: Some(vec!["openai:responses".to_string()]),
            endpoint_ids: None,
            operations: None,
        },
    ]);
    row
}

fn complete_codex_model_card(source_model_name: &str) -> serde_json::Value {
    json!({
        "id": source_model_name,
        "api_formats": ["openai:responses"],
        "slug": source_model_name,
        "display_name": "GPT-5.6-Sol",
        "description": "Frontier coding model",
        "default_reasoning_level": "low",
        "supported_reasoning_levels": [
            {"effort": "low", "description": "Low"},
            {"effort": "medium", "description": "Medium"},
            {"effort": "high", "description": "High"},
            {"effort": "xhigh", "description": "XHigh"},
            {"effort": "max", "description": "Max"},
            {"effort": "ultra", "description": "Ultra"}
        ],
        "shell_type": "shell_command",
        "visibility": "list",
        "supported_in_api": true,
        "priority": 1,
        "availability_nux": null,
        "upgrade": null,
        "base_instructions": "Use the current Codex instructions.",
        "model_messages": null,
        "support_verbosity": true,
        "default_verbosity": "low",
        "apply_patch_tool_type": "freeform",
        "truncation_policy": {"mode": "tokens", "limit": 10000},
        "supports_parallel_tool_calls": true,
        "experimental_supported_tools": [],
        "minimal_client_version": "0.144.0",
        "future_capability": {"enabled": true}
    })
}

struct PendingMinimalCandidateSelectionReadRepository;

impl PendingMinimalCandidateSelectionReadRepository {
    async fn pending_rows(
        &self,
    ) -> Result<Vec<StoredMinimalCandidateSelectionRow>, DataLayerError> {
        pending::<Result<Vec<StoredMinimalCandidateSelectionRow>, DataLayerError>>().await
    }
}

#[async_trait]
impl MinimalCandidateSelectionReadRepository for PendingMinimalCandidateSelectionReadRepository {
    async fn list_for_exact_api_format(
        &self,
        _api_format: &str,
    ) -> Result<Vec<StoredMinimalCandidateSelectionRow>, DataLayerError> {
        self.pending_rows().await
    }

    async fn list_for_exact_api_format_and_global_model(
        &self,
        _api_format: &str,
        _global_model_name: &str,
    ) -> Result<Vec<StoredMinimalCandidateSelectionRow>, DataLayerError> {
        self.pending_rows().await
    }

    async fn list_for_exact_api_format_and_requested_model(
        &self,
        _api_format: &str,
        _requested_model_name: &str,
    ) -> Result<Vec<StoredMinimalCandidateSelectionRow>, DataLayerError> {
        self.pending_rows().await
    }

    async fn list_for_exact_api_format_and_requested_model_page(
        &self,
        _query: &StoredRequestedModelCandidateRowsQuery,
    ) -> Result<Vec<StoredMinimalCandidateSelectionRow>, DataLayerError> {
        self.pending_rows().await
    }

}

struct CachedToggleMinimalCandidateSelectionReadRepository {
    row: StoredMinimalCandidateSelectionRow,
    active: AtomicBool,
    cached_rows_by_api_format: Mutex<HashMap<String, Vec<StoredMinimalCandidateSelectionRow>>>,
}

impl CachedToggleMinimalCandidateSelectionReadRepository {
    fn new(row: StoredMinimalCandidateSelectionRow) -> Self {
        Self {
            row,
            active: AtomicBool::new(true),
            cached_rows_by_api_format: Mutex::new(HashMap::new()),
        }
    }

    fn set_active(&self, active: bool) {
        self.active.store(active, Ordering::SeqCst);
    }

    fn rows_for_api_format(&self, api_format: &str) -> Vec<StoredMinimalCandidateSelectionRow> {
        let api_format = api_format.trim().to_string();
        let mut cached = self
            .cached_rows_by_api_format
            .lock()
            .expect("candidate row cache lock");
        if let Some(rows) = cached.get(&api_format) {
            return rows.clone();
        }

        let rows = if self.active.load(Ordering::SeqCst)
            && self
                .row
                .endpoint_api_format
                .eq_ignore_ascii_case(&api_format)
        {
            vec![self.row.clone()]
        } else {
            Vec::new()
        };
        cached.insert(api_format, rows.clone());
        rows
    }
}

#[async_trait]
impl MinimalCandidateSelectionReadRepository
    for CachedToggleMinimalCandidateSelectionReadRepository
{
    fn clear_local_cache(&self) {
        self.cached_rows_by_api_format
            .lock()
            .expect("candidate row cache lock")
            .clear();
    }

    async fn list_for_exact_api_format(
        &self,
        api_format: &str,
    ) -> Result<Vec<StoredMinimalCandidateSelectionRow>, DataLayerError> {
        Ok(self.rows_for_api_format(api_format))
    }

    async fn list_for_exact_api_format_and_global_model(
        &self,
        api_format: &str,
        global_model_name: &str,
    ) -> Result<Vec<StoredMinimalCandidateSelectionRow>, DataLayerError> {
        Ok(self
            .rows_for_api_format(api_format)
            .into_iter()
            .filter(|row| row.global_model_name == global_model_name)
            .collect())
    }

    async fn list_for_exact_api_format_and_requested_model(
        &self,
        api_format: &str,
        requested_model_name: &str,
    ) -> Result<Vec<StoredMinimalCandidateSelectionRow>, DataLayerError> {
        Ok(self
            .rows_for_api_format(api_format)
            .into_iter()
            .filter(|row| row.global_model_name == requested_model_name)
            .collect())
    }

    async fn list_for_exact_api_format_and_requested_model_page(
        &self,
        query: &StoredRequestedModelCandidateRowsQuery,
    ) -> Result<Vec<StoredMinimalCandidateSelectionRow>, DataLayerError> {
        Ok(self
            .rows_for_api_format(&query.api_format)
            .into_iter()
            .filter(|row| row.global_model_name == query.requested_model_name)
            .skip(query.offset as usize)
            .take(query.limit as usize)
            .collect())
    }

}

#[tokio::test]
async fn gateway_handles_public_openai_models_without_hitting_fallback_probe() {
    let fallback_probe_hits = Arc::new(Mutex::new(0usize));
    let fallback_probe_hits_clone = Arc::clone(&fallback_probe_hits);
    let fallback_probe = Router::new().route(
        "/{*path}",
        any(move |_request: Request| {
            let fallback_probe_hits_inner = Arc::clone(&fallback_probe_hits_clone);
            async move {
                *fallback_probe_hits_inner.lock().expect("mutex should lock") += 1;
                (StatusCode::OK, Body::from("proxied"))
            }
        }),
    );

    let auth_repository = Arc::new(InMemoryAuthApiKeySnapshotRepository::seed(vec![(
        Some(hash_api_key("sk-openai-models")),
        unrestricted_models_snapshot("key-1", "user-1"),
    )]));
    let candidate_repository =
        Arc::new(InMemoryMinimalCandidateSelectionReadRepository::seed(vec![
            sample_models_candidate_row("provider-openai", "openai", "openai:chat", "gpt-5", 10),
            sample_models_candidate_row("provider-openai", "openai", "openai:chat", "gpt-4.1", 10),
        ]));

    let (_unused_fallback_probe_url, fallback_probe_handle) = start_server(fallback_probe).await;
    let gateway = build_router_with_state(
        AppState::new()
            .expect("gateway should build")
            .with_data_state_for_tests(
                crate::data::GatewayDataState::with_minimal_candidate_selection_and_auth_for_tests(
                    candidate_repository,
                    auth_repository,
                ),
            ),
    );
    let (gateway_url, gateway_handle) = start_server(gateway).await;

    let response = reqwest::Client::new()
        .get(format!("{gateway_url}/v1/models"))
        .header("authorization", "Bearer sk-openai-models")
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::OK);
    let payload: serde_json::Value = response.json().await.expect("json body should parse");
    assert_eq!(payload["object"], "list");
    assert_eq!(payload["data"][0]["id"], "gpt-4.1");
    assert_eq!(payload["data"][1]["id"], "gpt-5");
    assert_eq!(payload["data"][0]["owned_by"], "aether");
    assert_eq!(*fallback_probe_hits.lock().expect("mutex should lock"), 0);

    gateway_handle.abort();
    fallback_probe_handle.abort();
}

#[tokio::test]
async fn gateway_serves_codex_model_cards_for_versioned_models_requests() {
    let codex_row =
        sample_codex_models_candidate_row("provider-codex-models", "frontier-sol", "gpt-5.6-sol");
    let incomplete_codex_row = sample_codex_models_candidate_row(
        "provider-codex-incomplete",
        "broken-luna",
        "gpt-5.6-luna",
    );
    let candidate_repository =
        Arc::new(InMemoryMinimalCandidateSelectionReadRepository::seed(vec![
            codex_row.clone(),
            incomplete_codex_row.clone(),
            sample_models_candidate_row(
                "provider-openai-responses",
                "openai",
                "openai:responses",
                "custom-responses-model",
                20,
            ),
        ]));
    let auth_repository = Arc::new(InMemoryAuthApiKeySnapshotRepository::seed(vec![
        (
            Some(hash_api_key("sk-codex-models")),
            codex_models_snapshot("key-codex-models", "user-codex-models"),
        ),
        (
            Some(hash_api_key("sk-standard-models")),
            unrestricted_models_snapshot("key-standard-models", "user-standard-models"),
        ),
    ]));
    let state = AppState::new()
        .expect("gateway should build")
        .with_data_state_for_tests(
            crate::data::GatewayDataState::with_minimal_candidate_selection_and_auth_for_tests(
                candidate_repository,
                auth_repository,
            ),
        );
    state
        .runtime_kv_setex(
            &format!(
                "upstream_models:{}:{}",
                codex_row.provider_id, codex_row.key_id
            ),
            &serde_json::to_string(&vec![complete_codex_model_card("gpt-5.6-sol")])
                .expect("model cache should serialize"),
            60,
        )
        .await
        .expect("model cache should seed");
    state
        .runtime_kv_setex(
            &format!(
                "upstream_models:{}:{}",
                incomplete_codex_row.provider_id, incomplete_codex_row.key_id
            ),
            &serde_json::to_string(&vec![json!({
                "id": "gpt-5.6-luna",
                "slug": "gpt-5.6-luna",
                "display_name": "GPT-5.6-Luna"
            })])
            .expect("incomplete model cache should serialize"),
            60,
        )
        .await
        .expect("incomplete model cache should seed");

    let gateway = build_router_with_state(state);
    let (gateway_url, gateway_handle) = start_server(gateway).await;
    let client = reqwest::Client::new();

    let codex_response = client
        .get(format!("{gateway_url}/v1/models?client_version=0.144.1"))
        .header("authorization", "Bearer sk-codex-models")
        .send()
        .await
        .expect("Codex models request should succeed");
    assert_eq!(codex_response.status(), StatusCode::OK);
    let codex_payload: serde_json::Value = codex_response
        .json()
        .await
        .expect("Codex models body should parse");
    assert_eq!(codex_payload["models"].as_array().map(Vec::len), Some(1));
    assert_eq!(codex_payload["models"][0]["slug"], "frontier-sol");
    assert_eq!(
        codex_payload["models"][0]["supported_reasoning_levels"][5]["effort"],
        "ultra"
    );
    assert_eq!(
        codex_payload["models"][0]["future_capability"],
        json!({"enabled": true})
    );
    assert!(codex_payload["models"][0].get("id").is_none());
    assert!(codex_payload["models"][0].get("api_formats").is_none());
    assert!(codex_payload.get("object").is_none());

    let standard_response = client
        .get(format!("{gateway_url}/v1/models"))
        .header("authorization", "Bearer sk-standard-models")
        .send()
        .await
        .expect("standard models request should succeed");
    assert_eq!(standard_response.status(), StatusCode::OK);
    let standard_payload: serde_json::Value = standard_response
        .json()
        .await
        .expect("standard models body should parse");
    assert_eq!(standard_payload["object"], "list");
    assert!(standard_payload["data"].is_array());
    assert!(standard_payload.get("models").is_none());

    gateway_handle.abort();
}

#[tokio::test]
async fn gateway_openai_models_list_drops_disabled_global_model_after_cache_invalidation() {
    let auth_repository = Arc::new(InMemoryAuthApiKeySnapshotRepository::seed(vec![(
        Some(hash_api_key("sk-openai-models-cache")),
        unrestricted_models_snapshot("key-models-cache", "user-models-cache"),
    )]));
    let row = sample_models_candidate_row(
        "provider-openai-cache",
        "openai",
        "openai:chat",
        "gpt-5",
        10,
    );
    let global_model_id = row.global_model_id.clone();
    let candidate_repository = Arc::new(CachedToggleMinimalCandidateSelectionReadRepository::new(
        row.clone(),
    ));
    let global_model_repository = Arc::new(
        InMemoryGlobalModelReadRepository::seed(Vec::new()).with_admin_global_models(vec![
            StoredAdminGlobalModel::new(
                global_model_id.clone(),
                row.global_model_name.clone(),
                "GPT 5".to_string(),
                true,
                None,
                None,
                None,
                None,
                0,
                1,
                0,
                Some(1_711_000_000),
                Some(1_711_000_000),
            )
            .expect("global model should build"),
        ]),
    );
    let state = AppState::new()
        .expect("gateway should build")
        .with_data_state_for_tests(
            crate::data::GatewayDataState::with_minimal_candidate_selection_and_auth_for_tests(
                candidate_repository.clone(),
                auth_repository,
            )
            .with_global_model_repository_for_tests(global_model_repository),
        );
    let gateway = build_router_with_state(state.clone());
    let (gateway_url, gateway_handle) = start_server(gateway).await;
    let client = reqwest::Client::new();

    let response = client
        .get(format!("{gateway_url}/v1/models"))
        .header("authorization", "Bearer sk-openai-models-cache")
        .send()
        .await
        .expect("initial models request should succeed");
    assert_eq!(response.status(), StatusCode::OK);
    let payload: serde_json::Value = response.json().await.expect("json body should parse");
    assert_eq!(payload["data"][0]["id"], "gpt-5");

    candidate_repository.set_active(false);
    let disabled_global_model = UpdateAdminGlobalModelRecord::new(
        global_model_id,
        "GPT 5".to_string(),
        false,
        None,
        None,
        None,
        None,
    )
    .expect("global model update record should build");
    state
        .update_admin_global_model(&disabled_global_model)
        .await
        .expect("global model update should succeed")
        .expect("global model should update");

    let response = client
        .get(format!("{gateway_url}/v1/models"))
        .header("authorization", "Bearer sk-openai-models-cache")
        .send()
        .await
        .expect("models request after disable should succeed");
    assert_eq!(response.status(), StatusCode::OK);
    let payload: serde_json::Value = response.json().await.expect("json body should parse");
    assert_eq!(
        payload["data"]
            .as_array()
            .expect("data should be an array")
            .len(),
        0
    );

    gateway_handle.abort();
}

#[tokio::test]
async fn gateway_returns_empty_openai_models_when_candidate_rows_stall() {
    let fallback_probe_hits = Arc::new(Mutex::new(0usize));
    let fallback_probe_hits_clone = Arc::clone(&fallback_probe_hits);
    let fallback_probe = Router::new().route(
        "/{*path}",
        any(move |_request: Request| {
            let fallback_probe_hits_inner = Arc::clone(&fallback_probe_hits_clone);
            async move {
                *fallback_probe_hits_inner.lock().expect("mutex should lock") += 1;
                (StatusCode::OK, Body::from("proxied"))
            }
        }),
    );

    let auth_repository = Arc::new(InMemoryAuthApiKeySnapshotRepository::seed(vec![(
        Some(hash_api_key("sk-openai-models-stalled")),
        unrestricted_models_snapshot("key-stalled", "user-stalled"),
    )]));
    let candidate_repository = Arc::new(PendingMinimalCandidateSelectionReadRepository);

    let (_unused_fallback_probe_url, fallback_probe_handle) = start_server(fallback_probe).await;
    let gateway = build_router_with_state(
        AppState::new()
            .expect("gateway should build")
            .with_data_state_for_tests(
                crate::data::GatewayDataState::with_minimal_candidate_selection_and_auth_for_tests(
                    candidate_repository,
                    auth_repository,
                ),
            ),
    );
    let (gateway_url, gateway_handle) = start_server(gateway).await;

    let response = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(500))
        .build()
        .expect("client should build")
        .get(format!("{gateway_url}/v1/models"))
        .header("authorization", "Bearer sk-openai-models-stalled")
        .send()
        .await
        .expect("request should return before client timeout");

    assert_eq!(response.status(), StatusCode::OK);
    let payload: serde_json::Value = response.json().await.expect("json body should parse");
    assert_eq!(payload["object"], "list");
    assert_eq!(
        payload["data"]
            .as_array()
            .expect("data should be an array")
            .len(),
        0
    );
    assert_eq!(*fallback_probe_hits.lock().expect("mutex should lock"), 0);

    gateway_handle.abort();
    fallback_probe_handle.abort();
}

#[tokio::test]
async fn gateway_returns_not_found_for_openai_model_detail_when_candidate_rows_stall() {
    let fallback_probe_hits = Arc::new(Mutex::new(0usize));
    let fallback_probe_hits_clone = Arc::clone(&fallback_probe_hits);
    let fallback_probe = Router::new().route(
        "/{*path}",
        any(move |_request: Request| {
            let fallback_probe_hits_inner = Arc::clone(&fallback_probe_hits_clone);
            async move {
                *fallback_probe_hits_inner.lock().expect("mutex should lock") += 1;
                (StatusCode::OK, Body::from("proxied"))
            }
        }),
    );

    let auth_repository = Arc::new(InMemoryAuthApiKeySnapshotRepository::seed(vec![(
        Some(hash_api_key("sk-openai-model-detail-stalled")),
        unrestricted_models_snapshot("key-detail-stalled", "user-detail-stalled"),
    )]));
    let candidate_repository = Arc::new(PendingMinimalCandidateSelectionReadRepository);

    let (_unused_fallback_probe_url, fallback_probe_handle) = start_server(fallback_probe).await;
    let gateway = build_router_with_state(
        AppState::new()
            .expect("gateway should build")
            .with_data_state_for_tests(
                crate::data::GatewayDataState::with_minimal_candidate_selection_and_auth_for_tests(
                    candidate_repository,
                    auth_repository,
                ),
            ),
    );
    let (gateway_url, gateway_handle) = start_server(gateway).await;

    let response = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(500))
        .build()
        .expect("client should build")
        .get(format!("{gateway_url}/v1/models/gpt-stalled"))
        .header("authorization", "Bearer sk-openai-model-detail-stalled")
        .send()
        .await
        .expect("request should return before client timeout");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let payload: serde_json::Value = response.json().await.expect("json body should parse");
    assert_eq!(payload["error"]["code"], "model_not_found");
    assert_eq!(*fallback_probe_hits.lock().expect("mutex should lock"), 0);

    gateway_handle.abort();
    fallback_probe_handle.abort();
}

#[tokio::test]
async fn gateway_handles_public_openai_models_with_cross_format_candidates_without_hitting_fallback_probe(
) {
    let fallback_probe_hits = Arc::new(Mutex::new(0usize));
    let fallback_probe_hits_clone = Arc::clone(&fallback_probe_hits);
    let fallback_probe = Router::new().route(
        "/{*path}",
        any(move |_request: Request| {
            let fallback_probe_hits_inner = Arc::clone(&fallback_probe_hits_clone);
            async move {
                *fallback_probe_hits_inner.lock().expect("mutex should lock") += 1;
                (StatusCode::OK, Body::from("proxied"))
            }
        }),
    );

    let auth_repository = Arc::new(InMemoryAuthApiKeySnapshotRepository::seed(vec![(
        Some(hash_api_key("sk-openai-models-cross-format")),
        unrestricted_models_snapshot("key-1", "user-1"),
    )]));
    let candidate_repository =
        Arc::new(InMemoryMinimalCandidateSelectionReadRepository::seed(vec![
            sample_models_candidate_row(
                "provider-claude",
                "claude",
                "claude:messages",
                "claude-3-7-sonnet",
                10,
            ),
        ]));

    let (_unused_fallback_probe_url, fallback_probe_handle) = start_server(fallback_probe).await;
    let gateway = build_router_with_state(
        AppState::new()
            .expect("gateway should build")
            .with_data_state_for_tests(
                crate::data::GatewayDataState::with_minimal_candidate_selection_and_auth_for_tests(
                    candidate_repository,
                    auth_repository,
                ),
            ),
    );
    let (gateway_url, gateway_handle) = start_server(gateway).await;

    let client = reqwest::Client::new();
    let list_response = client
        .get(format!("{gateway_url}/v1/models"))
        .header("authorization", "Bearer sk-openai-models-cross-format")
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(list_response.status(), StatusCode::OK);
    let list_payload: serde_json::Value =
        list_response.json().await.expect("json body should parse");
    assert_eq!(list_payload["object"], "list");
    assert_eq!(list_payload["data"][0]["id"], "claude-3-7-sonnet");
    assert_eq!(list_payload["data"][0]["owned_by"], "aether");

    let detail_response = client
        .get(format!("{gateway_url}/v1/models/claude-3-7-sonnet"))
        .header("authorization", "Bearer sk-openai-models-cross-format")
        .send()
        .await
        .expect("request should succeed");
    assert_eq!(detail_response.status(), StatusCode::OK);
    let detail_payload: serde_json::Value = detail_response
        .json()
        .await
        .expect("json body should parse");
    assert_eq!(detail_payload["id"], "claude-3-7-sonnet");
    assert_eq!(detail_payload["owned_by"], "aether");

    assert_eq!(*fallback_probe_hits.lock().expect("mutex should lock"), 0);

    gateway_handle.abort();
    fallback_probe_handle.abort();
}

#[tokio::test]
async fn gateway_handles_public_claude_models_without_hitting_fallback_probe() {
    let fallback_probe_hits = Arc::new(Mutex::new(0usize));
    let fallback_probe_hits_clone = Arc::clone(&fallback_probe_hits);
    let fallback_probe = Router::new().route(
        "/{*path}",
        any(move |_request: Request| {
            let fallback_probe_hits_inner = Arc::clone(&fallback_probe_hits_clone);
            async move {
                *fallback_probe_hits_inner.lock().expect("mutex should lock") += 1;
                (StatusCode::OK, Json(json!({"proxied": true}))).into_response()
            }
        }),
    );

    let auth_repository = Arc::new(InMemoryAuthApiKeySnapshotRepository::seed(vec![(
        Some(hash_api_key("sk-claude-models")),
        unrestricted_models_snapshot("key-claude", "user-claude"),
    )]));
    let candidate_repository =
        Arc::new(InMemoryMinimalCandidateSelectionReadRepository::seed(vec![
            sample_models_candidate_row(
                "provider-claude",
                "claude",
                "claude:messages",
                "claude-3-7-sonnet",
                10,
            ),
            sample_models_candidate_row(
                "provider-claude",
                "claude",
                "claude:messages",
                "claude-3-5-haiku",
                10,
            ),
        ]));

    let (_unused_fallback_probe_url, fallback_probe_handle) = start_server(fallback_probe).await;
    let gateway = build_router_with_state(
        AppState::new()
            .expect("gateway should build")
            .with_data_state_for_tests(
                crate::data::GatewayDataState::with_minimal_candidate_selection_and_auth_for_tests(
                    candidate_repository,
                    auth_repository,
                ),
            ),
    );
    let (gateway_url, gateway_handle) = start_server(gateway).await;

    let response = reqwest::Client::new()
        .get(format!("{gateway_url}/v1/models?limit=1"))
        .header("x-api-key", "sk-claude-models")
        .header("anthropic-version", "2023-06-01")
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::OK);
    let payload: serde_json::Value = response.json().await.expect("json body should parse");
    assert_eq!(payload["data"][0]["id"], "claude-3-5-haiku");
    assert_eq!(payload["first_id"], "claude-3-5-haiku");
    assert_eq!(payload["last_id"], "claude-3-5-haiku");
    assert_eq!(payload["has_more"], true);
    assert_eq!(*fallback_probe_hits.lock().expect("mutex should lock"), 0);

    gateway_handle.abort();
    fallback_probe_handle.abort();
}

#[tokio::test]
async fn gateway_handles_public_gemini_models_without_hitting_fallback_probe() {
    let fallback_probe_hits = Arc::new(Mutex::new(0usize));
    let fallback_probe_hits_clone = Arc::clone(&fallback_probe_hits);
    let fallback_probe = Router::new().route(
        "/{*path}",
        any(move |_request: Request| {
            let fallback_probe_hits_inner = Arc::clone(&fallback_probe_hits_clone);
            async move {
                *fallback_probe_hits_inner.lock().expect("mutex should lock") += 1;
                (StatusCode::OK, Json(json!({"proxied": true}))).into_response()
            }
        }),
    );

    let auth_repository = Arc::new(InMemoryAuthApiKeySnapshotRepository::seed(vec![(
        Some(hash_api_key("sk-gemini-models")),
        unrestricted_models_snapshot("key-gemini", "user-gemini"),
    )]));
    let candidate_repository =
        Arc::new(InMemoryMinimalCandidateSelectionReadRepository::seed(vec![
            sample_models_candidate_row(
                "provider-gemini",
                "gemini",
                "gemini:generate_content",
                "gemini-2.5-flash",
                10,
            ),
            sample_models_candidate_row(
                "provider-gemini",
                "gemini",
                "gemini:generate_content",
                "gemini-2.5-pro",
                10,
            ),
        ]));

    let (_unused_fallback_probe_url, fallback_probe_handle) = start_server(fallback_probe).await;
    let gateway = build_router_with_state(
        AppState::new()
            .expect("gateway should build")
            .with_data_state_for_tests(
                crate::data::GatewayDataState::with_minimal_candidate_selection_and_auth_for_tests(
                    candidate_repository,
                    auth_repository,
                ),
            ),
    );
    let (gateway_url, gateway_handle) = start_server(gateway).await;

    let response = reqwest::Client::new()
        .get(format!(
            "{gateway_url}/v1beta/models?pageSize=1&key=sk-gemini-models"
        ))
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::OK);
    let payload: serde_json::Value = response.json().await.expect("json body should parse");
    assert_eq!(payload["models"][0]["name"], "models/gemini-2.5-flash");
    assert_eq!(payload["nextPageToken"], "1");
    assert_eq!(*fallback_probe_hits.lock().expect("mutex should lock"), 0);

    gateway_handle.abort();
    fallback_probe_handle.abort();
}

#[tokio::test]
async fn gateway_does_not_locally_reject_image_model_name_on_chat_completions() {
    let fallback_probe_hits = Arc::new(Mutex::new(0usize));
    let fallback_probe_hits_clone = Arc::clone(&fallback_probe_hits);
    let fallback_probe = Router::new().route(
        "/{*path}",
        any(move |_request: Request| {
            let fallback_probe_hits_inner = Arc::clone(&fallback_probe_hits_clone);
            async move {
                *fallback_probe_hits_inner.lock().expect("mutex should lock") += 1;
                (StatusCode::OK, Json(json!({"proxied": true}))).into_response()
            }
        }),
    );

    let auth_repository = Arc::new(InMemoryAuthApiKeySnapshotRepository::seed(vec![(
        Some(hash_api_key("sk-openai-chat-image-model")),
        unrestricted_models_snapshot(
            "key-openai-chat-image-model",
            "user-openai-chat-image-model",
        ),
    )]));

    let (_unused_fallback_probe_url, fallback_probe_handle) = start_server(fallback_probe).await;
    let gateway = build_router_with_state(
        AppState::new()
            .expect("gateway should build")
            .with_auth_api_key_data_reader_for_tests(auth_repository),
    );
    let (gateway_url, gateway_handle) = start_server(gateway).await;

    let response = reqwest::Client::new()
        .post(format!("{gateway_url}/v1/chat/completions"))
        .header("authorization", "Bearer sk-openai-chat-image-model")
        .header(http::header::CONTENT_TYPE, "application/json")
        .body(
            serde_json::to_vec(&json!({
                "model": "gpt-image-2",
                "messages": [{"role": "user", "content": "hello"}]
            }))
            .expect("request body should encode"),
        )
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        response
            .headers()
            .get(EXECUTION_PATH_HEADER)
            .and_then(|value| value.to_str().ok()),
        Some(EXECUTION_PATH_LOCAL_EXECUTION_RUNTIME_MISS)
    );
    assert_eq!(*fallback_probe_hits.lock().expect("mutex should lock"), 0);

    gateway_handle.abort();
    fallback_probe_handle.abort();
}

#[tokio::test]
async fn gateway_rejects_image_request_above_gateway_limit_without_hitting_fallback_probe() {
    let fallback_probe_hits = Arc::new(Mutex::new(0usize));
    let fallback_probe_hits_clone = Arc::clone(&fallback_probe_hits);
    let fallback_probe = Router::new().route(
        "/{*path}",
        any(move |_request: Request| {
            let fallback_probe_hits_inner = Arc::clone(&fallback_probe_hits_clone);
            async move {
                *fallback_probe_hits_inner.lock().expect("mutex should lock") += 1;
                (StatusCode::OK, Json(json!({"proxied": true}))).into_response()
            }
        }),
    );

    let auth_repository = Arc::new(InMemoryAuthApiKeySnapshotRepository::seed(vec![(
        Some(hash_api_key("sk-openai-image-n")),
        unrestricted_models_snapshot("key-openai-image-n", "user-openai-image-n"),
    )]));

    let (_unused_fallback_probe_url, fallback_probe_handle) = start_server(fallback_probe).await;
    let gateway = build_router_with_state(
        AppState::new()
            .expect("gateway should build")
            .with_auth_api_key_data_reader_for_tests(auth_repository),
    );
    let (gateway_url, gateway_handle) = start_server(gateway).await;

    let response = reqwest::Client::new()
        .post(format!("{gateway_url}/v1/images/generations"))
        .header("authorization", "Bearer sk-openai-image-n")
        .header(http::header::CONTENT_TYPE, "application/json")
        .body(
            serde_json::to_vec(&json!({
                "model": "image-generation-model",
                "prompt": "draw",
                "n": openai_image_gateway_max_generation_count() + 1,
                "response_format": "b64_json"
            }))
            .expect("request body should encode"),
        )
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        response
            .headers()
            .get(EXECUTION_PATH_HEADER)
            .and_then(|value| value.to_str().ok()),
        Some(EXECUTION_PATH_LOCAL_AI_PUBLIC)
    );
    let payload: serde_json::Value = response.json().await.expect("json body should parse");
    assert_eq!(
        payload["detail"],
        format!(
            "当前图片反代仅支持 n=1..{}",
            openai_image_gateway_max_generation_count()
        )
    );
    assert_eq!(*fallback_probe_hits.lock().expect("mutex should lock"), 0);

    gateway_handle.abort();
    fallback_probe_handle.abort();
}

#[tokio::test]
async fn gateway_does_not_mount_image_variation_route_without_hitting_fallback_probe() {
    let fallback_probe_hits = Arc::new(Mutex::new(0usize));
    let fallback_probe_hits_clone = Arc::clone(&fallback_probe_hits);
    let fallback_probe = Router::new().route(
        "/{*path}",
        any(move |_request: Request| {
            let fallback_probe_hits_inner = Arc::clone(&fallback_probe_hits_clone);
            async move {
                *fallback_probe_hits_inner.lock().expect("mutex should lock") += 1;
                (StatusCode::OK, Json(json!({"proxied": true}))).into_response()
            }
        }),
    );

    let auth_repository = Arc::new(InMemoryAuthApiKeySnapshotRepository::seed(vec![(
        Some(hash_api_key("sk-openai-image-variation")),
        unrestricted_models_snapshot("key-openai-image-variation", "user-openai-image-variation"),
    )]));

    let (_unused_fallback_probe_url, fallback_probe_handle) = start_server(fallback_probe).await;
    let gateway = build_router_with_state(
        AppState::new()
            .expect("gateway should build")
            .with_auth_api_key_data_reader_for_tests(auth_repository),
    );
    let (gateway_url, gateway_handle) = start_server(gateway).await;

    let response = reqwest::Client::new()
        .post(format!("{gateway_url}/v1/images/variations"))
        .header("authorization", "Bearer sk-openai-image-variation")
        .header(http::header::CONTENT_TYPE, "application/json")
        .body(
            serde_json::to_vec(&json!({
                "model": "dall-e-2",
                "response_format": "url"
            }))
            .expect("request body should encode"),
        )
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(*fallback_probe_hits.lock().expect("mutex should lock"), 0);

    gateway_handle.abort();
    fallback_probe_handle.abort();
}
