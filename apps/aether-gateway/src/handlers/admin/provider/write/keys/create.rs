use crate::handlers::admin::provider::shared::payloads::AdminProviderKeyCreateRequest;
use crate::handlers::admin::provider::write::normalize::{
    normalize_api_format_list, normalize_auth_type, normalize_max_probe_interval_minutes,
    normalize_rate_multipliers,
};
use crate::handlers::admin::request::AdminAppState;
use crate::handlers::admin::shared::{
    decrypt_catalog_secret_with_fallbacks, encrypt_catalog_secret_with_fallbacks,
    normalize_json_object, normalize_string_list,
};
use crate::handlers::shared::normalize_optional_api_key_concurrent_limit;
use aether_data_contracts::repository::provider_catalog::{
    StoredProviderCatalogKey, StoredProviderCatalogProvider,
};
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub(crate) async fn build_admin_create_provider_key_record(
    state: &AdminAppState<'_>,
    provider: &StoredProviderCatalogProvider,
    payload: AdminProviderKeyCreateRequest,
) -> Result<StoredProviderCatalogKey, String> {
    let state = state.as_ref();
    let name = payload.name.trim();
    if name.is_empty() {
        return Err("name 为必填字段".to_string());
    }

    let api_formats = normalize_api_format_list(
        normalize_string_list(payload.api_formats)
            .ok_or_else(|| "api_formats 为必填字段".to_string())?,
    );
    let auth_type = normalize_auth_type(payload.auth_type.as_deref())?;
    let api_key = payload.api_key.unwrap_or_default().trim().to_string();

    let existing_keys = state
        .list_provider_catalog_keys_by_provider_ids(std::slice::from_ref(&provider.id))
        .await
        .map_err(|err| format!("{err:?}"))?;

    if !api_key.is_empty() {
        for existing in existing_keys
            .iter()
            .filter(|existing| raw_secret_auth_type(&existing.auth_type))
        {
            let Some(decrypted) = existing
                .encrypted_api_key
                .as_deref()
                .and_then(|ciphertext| {
                    decrypt_catalog_secret_with_fallbacks(state.encryption_key(), ciphertext)
                })
            else {
                continue;
            };
            if decrypted != "__placeholder__" && decrypted == api_key {
                return Err(format!(
                    "该 API Key 已存在于当前 Provider 中（名称: {}）",
                    existing.name
                ));
            }
        }
    }

    let encrypted_api_key = if api_key.is_empty() {
        None
    } else {
        Some(
            encrypt_catalog_secret_with_fallbacks(state, &api_key)
                .ok_or_else(|| "gateway 未配置 provider key 加密密钥".to_string())?,
        )
    };

    let now_unix_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let mut key = StoredProviderCatalogKey::new(
        Uuid::new_v4().to_string(),
        provider.id.clone(),
        name.to_string(),
        auth_type,
        normalize_json_object(payload.capabilities, "capabilities")?,
        true,
    )
    .map_err(|err| err.to_string())?
    .with_transport_fields(
        Some(json!(api_formats)),
        encrypted_api_key,
        normalize_rate_multipliers(payload.rate_multipliers)?,
        None,
        normalize_string_list(payload.allowed_models).map(|value| json!(value)),
        None,
        normalize_json_object(payload.fingerprint, "fingerprint")?,
    )
    .map_err(|err| err.to_string())?;
    key.note = payload
        .note
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    key.internal_priority = payload.internal_priority.unwrap_or(50);
    key.rpm_limit = payload.rpm_limit;
    key.concurrent_limit = normalize_optional_api_key_concurrent_limit(payload.concurrent_limit)?;
    key.cache_ttl_minutes = payload.cache_ttl_minutes.unwrap_or(5);
    key.max_probe_interval_minutes =
        normalize_max_probe_interval_minutes(payload.max_probe_interval_minutes.unwrap_or(32))?;
    key.request_count = Some(0);
    key.success_count = Some(0);
    key.error_count = Some(0);
    key.total_response_time_ms = Some(0);
    key.auto_fetch_models = payload.auto_fetch_models.unwrap_or(false);
    key.locked_models = normalize_string_list(payload.locked_models).map(|value| json!(value));
    key.model_include_patterns =
        normalize_string_list(payload.model_include_patterns).map(|value| json!(value));
    key.model_exclude_patterns =
        normalize_string_list(payload.model_exclude_patterns).map(|value| json!(value));
    key.health_by_format = Some(json!({}));
    key.circuit_breaker_by_format = Some(json!({}));
    key.created_at_unix_ms = Some(now_unix_secs);
    key.updated_at_unix_secs = Some(now_unix_secs);
    Ok(key)
}

fn raw_secret_auth_type(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "api_key" | "bearer"
    )
}
