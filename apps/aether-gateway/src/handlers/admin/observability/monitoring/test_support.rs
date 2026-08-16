use crate::control::GatewayPublicRequestContext;
use aether_crypto::{encrypt_fernet_plaintext, DEVELOPMENT_ENCRYPTION_KEY};
use aether_data_contracts::repository::{
    candidates::{RequestCandidateStatus, StoredRequestCandidate},
    provider_catalog::{
        StoredProviderCatalogEndpoint, StoredProviderCatalogKey, StoredProviderCatalogProvider,
    },
    usage::StoredRequestUsageAudit,
};
use axum::http::{self, Uri};
use serde_json::json;

use aether_data::repository::auth::StoredAuthApiKeyExportRecord;
use aether_data::repository::users::{StoredUserAuthRecord, StoredUserExportRow};

pub(super) fn request_context(method: http::Method, uri: &str) -> GatewayPublicRequestContext {
    GatewayPublicRequestContext::from_request_parts(
        "trace-123",
        &method,
        &uri.parse::<Uri>().expect("uri should parse"),
        &http::HeaderMap::new(),
        None,
    )
}

pub(super) fn sample_usage(
    request_id: &str,
    provider_id: &str,
    provider_name: &str,
    total_tokens: i32,
    total_cost_usd: f64,
    status: &str,
    status_code: Option<i32>,
    created_at_unix_ms: i64,
) -> StoredRequestUsageAudit {
    let is_error = status_code.is_some_and(|value| value >= 400)
        || status.trim().eq_ignore_ascii_case("failed")
        || status.trim().eq_ignore_ascii_case("error");
    StoredRequestUsageAudit::new(
        format!("usage-{request_id}"),
        request_id.to_string(),
        Some("user-1".to_string()),
        Some("api-key-1".to_string()),
        Some("alice".to_string()),
        Some("monitoring-key".to_string()),
        provider_name.to_string(),
        "gpt-4.1".to_string(),
        None,
        Some(provider_id.to_string()),
        Some("endpoint-1".to_string()),
        Some("provider-key-1".to_string()),
        Some("chat".to_string()),
        Some("openai:chat".to_string()),
        Some("openai".to_string()),
        Some("chat".to_string()),
        Some("openai:chat".to_string()),
        Some("openai".to_string()),
        Some("chat".to_string()),
        false,
        false,
        total_tokens / 2,
        total_tokens / 2,
        total_tokens,
        total_cost_usd,
        total_cost_usd,
        status_code,
        is_error.then(|| "boom".to_string()),
        is_error.then(|| "upstream_error".to_string()),
        Some(120),
        Some(30),
        status.to_string(),
        "billed".to_string(),
        created_at_unix_ms,
        created_at_unix_ms,
        Some(created_at_unix_ms),
    )
    .expect("usage should build")
}

pub(super) fn sample_candidate(
    id: &str,
    request_id: &str,
    candidate_index: i32,
    status: RequestCandidateStatus,
    started_at_unix_ms: Option<i64>,
    latency_ms: Option<i32>,
    status_code: Option<i32>,
) -> StoredRequestCandidate {
    StoredRequestCandidate::new(
        id.to_string(),
        request_id.to_string(),
        Some("user-1".to_string()),
        Some("api-key-1".to_string()),
        Some("alice".to_string()),
        Some("default".to_string()),
        candidate_index,
        0,
        Some("provider-1".to_string()),
        Some("endpoint-1".to_string()),
        Some("provider-key-1".to_string()),
        status,
        None,
        false,
        status_code,
        None,
        None,
        latency_ms,
        Some(1),
        None,
        Some(json!({"cache_1h": true})),
        (100 + i64::from(candidate_index)) * 1_000,
        started_at_unix_ms.map(|v| v * 1_000),
        started_at_unix_ms.map(|value| (value + 1) * 1_000),
    )
    .expect("candidate should build")
}

pub(super) fn sample_provider() -> StoredProviderCatalogProvider {
    StoredProviderCatalogProvider::new(
        "provider-1".to_string(),
        "OpenAI".to_string(),
        Some("https://openai.com".to_string()),
        "custom".to_string(),
    )
    .expect("provider should build")
}

pub(super) fn sample_inactive_provider() -> StoredProviderCatalogProvider {
    StoredProviderCatalogProvider::new(
        "provider-2".to_string(),
        "Anthropic".to_string(),
        Some("https://anthropic.com".to_string()),
        "custom".to_string(),
    )
    .expect("provider should build")
    .with_transport_fields(false, false, false, None, None, None, None, None, None)
}

pub(super) fn sample_endpoint() -> StoredProviderCatalogEndpoint {
    StoredProviderCatalogEndpoint::new(
        "endpoint-1".to_string(),
        "provider-1".to_string(),
        "openai:chat".to_string(),
        Some("openai".to_string()),
        Some("chat".to_string()),
        true,
    )
    .expect("endpoint should build")
}

pub(super) fn sample_key() -> StoredProviderCatalogKey {
    StoredProviderCatalogKey::new(
        "provider-key-1".to_string(),
        "provider-1".to_string(),
        "prod-key".to_string(),
        "api_key".to_string(),
        Some(json!({"cache_1h": true})),
        true,
    )
    .expect("key should build")
}

pub(super) fn sample_monitoring_auth_user(user_id: &str) -> StoredUserAuthRecord {
    StoredUserAuthRecord::new(
        user_id.to_string(),
        Some("alice@example.com".to_string()),
        true,
        "alice".to_string(),
        None,
        "user".to_string(),
        "local".to_string(),
        None,
        None,
        None,
        true,
        false,
        None,
        None,
    )
    .expect("auth user should build")
}

pub(super) fn sample_monitoring_export_user(user_id: &str) -> StoredUserExportRow {
    StoredUserExportRow::new(
        user_id.to_string(),
        Some("alice@example.com".to_string()),
        true,
        "alice".to_string(),
        None,
        "user".to_string(),
        "local".to_string(),
        None,
        None,
        None,
        None,
        None,
        true,
    )
    .expect("export user should build")
}

pub(super) fn sample_monitoring_export_api_key(
    user_id: &str,
    api_key_id: &str,
) -> StoredAuthApiKeyExportRecord {
    StoredAuthApiKeyExportRecord::new(
        user_id.to_string(),
        api_key_id.to_string(),
        format!("hash-{api_key_id}"),
        Some(
            encrypt_fernet_plaintext(DEVELOPMENT_ENCRYPTION_KEY, "sk-user-monitoring-1234")
                .expect("user key should encrypt"),
        ),
        Some("Alice Key".to_string()),
        None,
        None,
        None,
        None,
        None,
        None,
        true,
        None,
        false,
        0,
        0,
        0.0,
        false,
    )
    .expect("export api key should build")
}

pub(super) fn sample_monitoring_catalog_endpoint() -> StoredProviderCatalogEndpoint {
    sample_endpoint()
        .with_transport_fields(
            "https://api.openai.example/v1".to_string(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("endpoint transport fields should build")
}

pub(super) fn sample_monitoring_catalog_key() -> StoredProviderCatalogKey {
    sample_key()
        .with_transport_fields(
            None,
            encrypt_fernet_plaintext(
                DEVELOPMENT_ENCRYPTION_KEY,
                "sk-upstream-monitoring-5678",
            )
            .expect("provider key should encrypt"),
            None,
            Some(json!({"cache": 1.0})),
            None,
            None,
            None,
            None,
            None,
        )
        .expect("provider key transport fields should build")
}
