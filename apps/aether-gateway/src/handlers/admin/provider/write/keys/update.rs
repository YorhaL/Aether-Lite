use crate::handlers::admin::provider::shared::payloads::AdminProviderKeyUpdatePatch;
use crate::handlers::admin::provider::write::normalize::{
    normalize_api_format_json_object_keys, normalize_api_format_list, normalize_auth_type,
    normalize_max_probe_interval_minutes, normalize_rate_multipliers,
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

pub(crate) async fn build_admin_update_provider_key_record(
    state: &AdminAppState<'_>,
    provider: &StoredProviderCatalogProvider,
    existing: &StoredProviderCatalogKey,
    patch: AdminProviderKeyUpdatePatch,
) -> Result<StoredProviderCatalogKey, String> {
    let existing_keys = state
        .as_ref()
        .list_provider_catalog_keys_by_provider_ids(std::slice::from_ref(&provider.id))
        .await
        .map_err(|err| format!("{err:?}"))?;
    build_admin_update_provider_key_record_with_existing_keys(
        state,
        provider,
        existing,
        &existing_keys,
        patch,
    )
}

pub(crate) fn build_admin_update_provider_key_record_with_existing_keys(
    state: &AdminAppState<'_>,
    provider: &StoredProviderCatalogProvider,
    existing: &StoredProviderCatalogKey,
    existing_keys: &[StoredProviderCatalogKey],
    patch: AdminProviderKeyUpdatePatch,
) -> Result<StoredProviderCatalogKey, String> {
    let state = state.as_ref();
    let mut updated = existing.clone();
    let (fields, payload) = patch.into_parts();
    let auto_fetch_disabled =
        existing.auto_fetch_models && matches!(payload.auto_fetch_models, Some(false));
    let current_auth_type = normalize_auth_type(Some(&existing.auth_type))?;
    let target_auth_type = payload
        .auth_type
        .as_deref()
        .map(|value| normalize_auth_type(Some(value)))
        .transpose()?
        .unwrap_or_else(|| current_auth_type.clone());
    let api_key_present = fields.contains("api_key");
    let api_key_value = payload
        .api_key
        .as_deref()
        .map(str::trim)
        .map(ToOwned::to_owned);
    if let Some(api_key) = api_key_value
        .as_deref()
        .filter(|value| !value.is_empty() && *value != "__placeholder__")
    {
        for existing_key in existing_keys
            .iter()
            .filter(|key| key.id != existing.id && raw_secret_auth_type(&key.auth_type))
        {
            let Some(decrypted) =
                existing_key
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
                    existing_key.name
                ));
            }
        }
        updated.encrypted_api_key = Some(
            encrypt_catalog_secret_with_fallbacks(state, api_key)
                .ok_or_else(|| "gateway 未配置 provider key 加密密钥".to_string())?,
        );
    } else if api_key_present {
        updated.encrypted_api_key = None;
    }

    if fields.contains("api_formats") {
        let api_formats = normalize_api_format_list(
            normalize_string_list(payload.api_formats)
                .ok_or_else(|| "api_formats 为必填字段".to_string())?,
        );
        updated.api_formats = Some(json!(api_formats));
    }

    updated.auth_type = target_auth_type;

    if let Some(name) = payload.name {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err("name 为必填字段".to_string());
        }
        updated.name = trimmed.to_string();
    }
    if fields.contains("rate_multipliers") {
        updated.rate_multipliers = normalize_rate_multipliers(payload.rate_multipliers)?;
    }
    if let Some(internal_priority) = payload.internal_priority {
        updated.internal_priority = internal_priority;
    }
    if fields.contains("global_priority_by_format") {
        updated.global_priority_by_format = normalize_api_format_json_object_keys(
            payload.global_priority_by_format,
            "global_priority_by_format",
        )?;
    }
    if fields.contains("rpm_limit") {
        updated.rpm_limit = payload.rpm_limit;
        if payload.rpm_limit.is_none() {
            updated.learned_rpm_limit = None;
        }
    }
    if fields.contains("concurrent_limit") {
        updated.concurrent_limit =
            normalize_optional_api_key_concurrent_limit(payload.concurrent_limit)?;
    }
    if fields.contains("allowed_models") {
        updated.allowed_models =
            normalize_string_list(payload.allowed_models).map(|value| json!(value));
    }
    if fields.contains("capabilities") {
        updated.capabilities = normalize_json_object(payload.capabilities, "capabilities")?;
    }
    if let Some(cache_ttl_minutes) = payload.cache_ttl_minutes {
        updated.cache_ttl_minutes = cache_ttl_minutes;
    }
    if let Some(max_probe_interval_minutes) = payload.max_probe_interval_minutes {
        updated.max_probe_interval_minutes =
            normalize_max_probe_interval_minutes(max_probe_interval_minutes)?;
    }
    if let Some(is_active) = payload.is_active {
        updated.is_active = is_active;
    }
    if fields.contains("note") {
        updated.note = payload
            .note
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
    }
    if let Some(auto_fetch_models) = payload.auto_fetch_models {
        updated.auto_fetch_models = auto_fetch_models;
    }
    if auto_fetch_disabled && !fields.contains("allowed_models") {
        updated.allowed_models = None;
    }
    if fields.contains("locked_models") {
        updated.locked_models =
            normalize_string_list(payload.locked_models).map(|value| json!(value));
    }
    if fields.contains("model_include_patterns") {
        updated.model_include_patterns =
            normalize_string_list(payload.model_include_patterns).map(|value| json!(value));
    }
    if fields.contains("model_exclude_patterns") {
        updated.model_exclude_patterns =
            normalize_string_list(payload.model_exclude_patterns).map(|value| json!(value));
    }
    if fields.contains("fingerprint") {
        updated.fingerprint = normalize_json_object(payload.fingerprint, "fingerprint")?;
    }
    updated.updated_at_unix_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs());
    Ok(updated)
}

pub(crate) fn admin_provider_key_update_requires_immediate_model_fetch(
    existing: &StoredProviderCatalogKey,
    updated: &StoredProviderCatalogKey,
) -> bool {
    let filters_changed = existing.model_include_patterns != updated.model_include_patterns
        || existing.model_exclude_patterns != updated.model_exclude_patterns;
    let locked_models_changed = existing.locked_models != updated.locked_models;
    updated.auto_fetch_models
        && (!existing.auto_fetch_models || filters_changed || locked_models_changed)
}

fn raw_secret_auth_type(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "api_key" | "bearer"
    )
}
