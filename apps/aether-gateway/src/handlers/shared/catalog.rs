use std::borrow::Cow;

#[cfg(test)]
use aether_crypto::DEVELOPMENT_ENCRYPTION_KEY;
use aether_crypto::{decrypt_fernet_ciphertext, encrypt_fernet_plaintext};
use aether_data_contracts::repository::provider_catalog::StoredProviderCatalogKey;
use aether_scheduler_core::provider_key_circuit_payload_is_active_open_at;
use serde_json::{json, Value};

use crate::handlers::shared::{json_string_list, unix_secs_to_rfc3339};
use crate::provider_key_auth::provider_key_configured_api_formats;
use crate::AppState;

pub(crate) fn provider_catalog_key_supports_format(
    key: &StoredProviderCatalogKey,
    api_format: &str,
) -> bool {
    let expected = aether_ai_formats::normalize_api_format_alias(api_format);
    let formats = provider_key_configured_api_formats(key);
    formats.is_empty()
        || formats
            .iter()
            .any(|candidate| aether_ai_formats::normalize_api_format_alias(candidate) == expected)
}

pub(crate) fn decrypt_catalog_secret_with_fallbacks(
    encryption_key: Option<&str>,
    ciphertext: &str,
) -> Option<String> {
    let encryption_key = encryption_key.map(str::trim).unwrap_or("");
    if !encryption_key.is_empty() {
        if let Ok(value) = decrypt_fernet_ciphertext(encryption_key, ciphertext) {
            return Some(value);
        }
    }
    for env_key in ["AETHER_GATEWAY_DATA_ENCRYPTION_KEY", "ENCRYPTION_KEY"] {
        let Ok(fallback) = std::env::var(env_key) else {
            continue;
        };
        let fallback = fallback.trim();
        if fallback.is_empty() || fallback == encryption_key {
            continue;
        }
        if let Ok(value) = decrypt_fernet_ciphertext(fallback, ciphertext) {
            return Some(value);
        }
    }
    #[cfg(test)]
    if encryption_key != DEVELOPMENT_ENCRYPTION_KEY {
        if let Ok(value) = decrypt_fernet_ciphertext(DEVELOPMENT_ENCRYPTION_KEY, ciphertext) {
            return Some(value);
        }
    }
    None
}

pub(crate) fn effective_catalog_encryption_key(state: &AppState) -> Option<Cow<'_, str>> {
    let encryption_key = state.encryption_key().map(str::trim).unwrap_or("");
    if !encryption_key.is_empty() {
        return Some(Cow::Borrowed(encryption_key));
    }
    for env_key in ["AETHER_GATEWAY_DATA_ENCRYPTION_KEY", "ENCRYPTION_KEY"] {
        let Ok(candidate) = std::env::var(env_key) else {
            continue;
        };
        let trimmed = candidate.trim();
        if !trimmed.is_empty() {
            return Some(Cow::Owned(trimmed.to_string()));
        }
    }
    #[cfg(test)]
    {
        return Some(Cow::Borrowed(DEVELOPMENT_ENCRYPTION_KEY));
    }
    #[allow(unreachable_code)]
    None
}

pub(crate) fn encrypt_catalog_secret_with_fallbacks(
    state: &AppState,
    plaintext: &str,
) -> Option<String> {
    let encryption_key = effective_catalog_encryption_key(state)?;
    encrypt_fernet_plaintext(encryption_key.as_ref(), plaintext).ok()
}

pub(crate) fn take_secret_prefix(value: &str, prefix_chars: usize) -> &str {
    let end = value
        .char_indices()
        .nth(prefix_chars)
        .map(|(index, _)| index)
        .unwrap_or(value.len());
    &value[..end]
}

pub(crate) fn take_secret_suffix(value: &str, suffix_chars: usize) -> &str {
    if suffix_chars == 0 {
        return &value[value.len()..];
    }
    let start = value
        .char_indices()
        .rev()
        .nth(suffix_chars - 1)
        .map(|(index, _)| index)
        .unwrap_or(0);
    &value[start..]
}

pub(crate) fn masked_catalog_api_key(state: &AppState, key: &StoredProviderCatalogKey) -> String {
    let Some(ciphertext) = key
        .encrypted_api_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return "[未设置]".to_string();
    };
    decrypt_catalog_secret_with_fallbacks(state.encryption_key(), ciphertext)
        .map(|value| {
            if value.chars().count() <= 12 {
                format!("{value}***")
            } else {
                format!(
                    "{}***{}",
                    take_secret_prefix(&value, 8),
                    take_secret_suffix(&value, 4)
                )
            }
        })
        .unwrap_or_else(|| "***ERROR***".to_string())
}

pub(crate) fn masked_catalog_api_key_for_provider(
    state: &AppState,
    key: &StoredProviderCatalogKey,
) -> String {
    masked_catalog_api_key(state, key)
}

pub(crate) fn default_provider_key_status_snapshot() -> serde_json::Value {
    json!({})
}

pub(crate) fn provider_key_status_snapshot_payload(
    key: &StoredProviderCatalogKey,
) -> serde_json::Value {
    key.status_snapshot
        .as_ref()
        .filter(|value| value.is_object())
        .cloned()
        .unwrap_or_else(default_provider_key_status_snapshot)
}

pub(crate) fn provider_key_health_summary(
    key: &StoredProviderCatalogKey,
) -> (
    f64,
    i64,
    Option<String>,
    bool,
    serde_json::Map<String, serde_json::Value>,
) {
    provider_key_health_summary_with_circuit_predicate(key, |value| {
        value
            .get("open")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
    })
}

pub(crate) fn provider_key_health_summary_at(
    key: &StoredProviderCatalogKey,
    now_unix_secs: u64,
) -> (
    f64,
    i64,
    Option<String>,
    bool,
    serde_json::Map<String, serde_json::Value>,
) {
    provider_key_health_summary_with_circuit_predicate(key, |value| {
        provider_key_circuit_payload_is_active_open_at(value, now_unix_secs)
    })
}

fn provider_key_health_summary_with_circuit_predicate(
    key: &StoredProviderCatalogKey,
    circuit_is_open: impl Fn(&serde_json::Value) -> bool,
) -> (
    f64,
    i64,
    Option<String>,
    bool,
    serde_json::Map<String, serde_json::Value>,
) {
    let health_by_format = key
        .health_by_format
        .as_ref()
        .and_then(serde_json::Value::as_object)
        .cloned()
        .unwrap_or_default();
    let circuit_by_format = key
        .circuit_breaker_by_format
        .as_ref()
        .and_then(serde_json::Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut min_health_score = 1.0_f64;
    let mut max_consecutive = 0_i64;
    let mut last_failure_at: Option<String> = None;
    for value in health_by_format.values() {
        let score = value
            .get("health_score")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(1.0);
        min_health_score = min_health_score.min(score);
        max_consecutive = max_consecutive.max(
            value
                .get("consecutive_failures")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0),
        );
        if let Some(last_failure) = value
            .get("last_failure_at")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned)
        {
            if last_failure_at
                .as_ref()
                .is_none_or(|current| last_failure > *current)
            {
                last_failure_at = Some(last_failure);
            }
        }
    }
    (
        if health_by_format.is_empty() {
            1.0
        } else {
            min_health_score
        },
        max_consecutive,
        last_failure_at,
        circuit_by_format.values().any(circuit_is_open),
        circuit_by_format,
    )
}

pub(crate) fn build_admin_provider_key_response(
    state: &AppState,
    key: &StoredProviderCatalogKey,
    api_formats: &[String],
    now_unix_secs: u64,
) -> serde_json::Value {
    let request_count = u64::from(key.request_count.unwrap_or(0));
    let success_count = u64::from(key.success_count.unwrap_or(0));
    let error_count = u64::from(key.error_count.unwrap_or(0));
    let success_rate = if request_count > 0 {
        success_count as f64 / request_count as f64
    } else {
        0.0
    };
    let avg_response_time_ms = if success_count > 0 {
        key.total_response_time_ms.unwrap_or(0) as f64 / success_count as f64
    } else {
        0.0
    };
    let (
        health_score,
        consecutive_failures,
        last_failure_at,
        circuit_breaker_open,
        circuit_by_format,
    ) = provider_key_health_summary_at(key, now_unix_secs);
    let circuit_sample = circuit_by_format
        .values()
        .find(|value| provider_key_circuit_payload_is_active_open_at(value, now_unix_secs))
        .or_else(|| circuit_by_format.values().next());
    let is_adaptive = key.rpm_limit.is_none();
    let effective_limit = if is_adaptive {
        key.learned_rpm_limit
    } else {
        key.rpm_limit
    };

    json!({
        "id": key.id,
        "provider_id": key.provider_id,
        "api_formats": api_formats,
        "api_key_masked": masked_catalog_api_key_for_provider(state, key),
        "api_key_plain": Value::Null,
        "auth_type": key.auth_type,
        "name": key.name,
        "internal_priority": key.internal_priority,
        "rpm_limit": key.rpm_limit,
        "concurrent_limit": key.concurrent_limit,
        "allowed_models": json_string_list(key.allowed_models.as_ref()),
        "capabilities": key.capabilities,
        "status_snapshot": provider_key_status_snapshot_payload(key),
        "cache_ttl_minutes": key.cache_ttl_minutes,
        "max_probe_interval_minutes": key.max_probe_interval_minutes,
        "health_by_format": key.health_by_format,
        "circuit_breaker_by_format": key.circuit_breaker_by_format,
        "health_score": health_score,
        "consecutive_failures": consecutive_failures,
        "last_failure_at": last_failure_at,
        "circuit_breaker_open": circuit_breaker_open,
        "circuit_breaker_open_at": circuit_sample.and_then(|value| value.get("open_at")).cloned(),
        "next_probe_at": circuit_sample.and_then(|value| value.get("next_probe_at")).cloned(),
        "half_open_until": circuit_sample.and_then(|value| value.get("half_open_until")).cloned(),
        "request_count": request_count,
        "success_count": success_count,
        "error_count": error_count,
        "success_rate": success_rate,
        "avg_response_time_ms": avg_response_time_ms,
        "is_active": key.is_active,
        "is_adaptive": is_adaptive,
        "learned_rpm_limit": key.learned_rpm_limit,
        "effective_limit": effective_limit,
        "utilization_samples": key.utilization_samples,
        "last_probe_increase_at": key.last_probe_increase_at_unix_secs.and_then(unix_secs_to_rfc3339),
        "concurrent_429_count": key.concurrent_429_count,
        "rpm_429_count": key.rpm_429_count,
        "last_429_at": key.last_429_at_unix_secs.and_then(unix_secs_to_rfc3339),
        "last_429_type": key.last_429_type,
        "note": key.note,
        "auto_fetch_models": key.auto_fetch_models,
        "last_models_fetch_at": key.last_models_fetch_at_unix_secs.and_then(unix_secs_to_rfc3339),
        "last_models_fetch_error": key.last_models_fetch_error,
        "locked_models": key.locked_models,
        "model_include_patterns": key.model_include_patterns,
        "model_exclude_patterns": key.model_exclude_patterns,
        "last_used_at": key.last_used_at_unix_secs.and_then(unix_secs_to_rfc3339),
        "created_at": unix_secs_to_rfc3339(key.created_at_unix_ms.unwrap_or(now_unix_secs)),
        "updated_at": unix_secs_to_rfc3339(key.updated_at_unix_secs.unwrap_or(now_unix_secs)),
    })
}
