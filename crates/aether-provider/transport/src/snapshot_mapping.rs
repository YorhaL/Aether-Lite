use aether_crypto::{decrypt_fernet_ciphertext, looks_like_fernet_ciphertext};
use aether_data_contracts::repository::provider_catalog::{
    StoredProviderCatalogEndpoint, StoredProviderCatalogKey, StoredProviderCatalogProvider,
};
use aether_data_contracts::DataLayerError;

use super::{
    GatewayProviderTransportEndpoint, GatewayProviderTransportKey, GatewayProviderTransportProvider,
};

pub(super) fn map_provider(
    provider: StoredProviderCatalogProvider,
) -> GatewayProviderTransportProvider {
    GatewayProviderTransportProvider {
        id: provider.id,
        name: provider.name,
        is_active: provider.is_active,
        concurrent_limit: provider.concurrent_limit,
        max_retries: provider.max_retries,
        request_timeout_secs: provider.request_timeout_secs,
        stream_first_byte_timeout_secs: provider.stream_first_byte_timeout_secs,
        config: normalize_provider_config(provider.config),
    }
}

pub(super) fn map_endpoint(
    endpoint: StoredProviderCatalogEndpoint,
) -> GatewayProviderTransportEndpoint {
    GatewayProviderTransportEndpoint {
        id: endpoint.id,
        provider_id: endpoint.provider_id,
        api_format: endpoint.api_format,
        is_active: endpoint.is_active,
        base_url: endpoint.base_url,
        header_rules: normalize_optional_json(endpoint.header_rules),
        body_rules: normalize_optional_json(endpoint.body_rules),
        max_retries: endpoint.max_retries,
        custom_path: endpoint.custom_path,
        config: normalize_optional_json(endpoint.config),
    }
}

pub(super) fn map_key(
    key: StoredProviderCatalogKey,
    encryption_key: &str,
    fallback_encryption_keys: &[String],
) -> Result<GatewayProviderTransportKey, DataLayerError> {
    let decrypted_api_key = key
        .encrypted_api_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|ciphertext| decrypt_secret(encryption_key, fallback_encryption_keys, ciphertext))
        .transpose()?
        .unwrap_or_default();

    Ok(GatewayProviderTransportKey {
        id: key.id,
        provider_id: key.provider_id,
        name: key.name,
        auth_type: key.auth_type,
        is_active: key.is_active,
        api_formats: normalize_string_list(
            normalize_optional_json(key.api_formats),
            "provider_api_keys.api_formats",
        )?,
        allowed_models: normalize_string_list(
            normalize_optional_json(key.allowed_models),
            "provider_api_keys.allowed_models",
        )?,
        capabilities: normalize_optional_json(key.capabilities),
        rate_multipliers: normalize_optional_json(key.rate_multipliers),
        global_priority_by_format: normalize_optional_json(key.global_priority_by_format),
        expires_at_unix_secs: key.expires_at_unix_secs,
        fingerprint: normalize_optional_json(key.fingerprint),
        decrypted_api_key,
    })
}

fn normalize_optional_json(value: Option<serde_json::Value>) -> Option<serde_json::Value> {
    match value {
        Some(serde_json::Value::Null) | None => None,
        Some(value) => Some(value),
    }
}

fn normalize_provider_config(value: Option<serde_json::Value>) -> Option<serde_json::Value> {
    let mut config = normalize_optional_json(value)?.as_object()?.clone();
    config.retain(|key, _| matches!(key.as_str(), "chat_pii_redaction" | "failover_rules"));
    (!config.is_empty()).then_some(serde_json::Value::Object(config))
}

fn decrypt_secret(
    encryption_key: &str,
    fallback_encryption_keys: &[String],
    ciphertext: &str,
) -> Result<String, DataLayerError> {
    if !looks_like_fernet_ciphertext(ciphertext.trim()) {
        return Ok(ciphertext.trim().to_string());
    }

    match decrypt_fernet_ciphertext(encryption_key, ciphertext) {
        Ok(value) => Ok(value),
        Err(error) => {
            for fallback_encryption_key in fallback_encryption_keys {
                if let Ok(value) = decrypt_fernet_ciphertext(fallback_encryption_key, ciphertext) {
                    return Ok(value);
                }
            }
            Err(DataLayerError::UnexpectedValue(format!(
                "failed to decrypt provider_api_keys.api_key: {error}"
            )))
        }
    }
}

pub(super) fn fallback_encryption_keys(primary_encryption_key: &str) -> Vec<String> {
    let mut keys = Vec::new();
    for env_key in ["AETHER_GATEWAY_DATA_ENCRYPTION_KEY", "ENCRYPTION_KEY"] {
        let Ok(value) = std::env::var(env_key) else {
            continue;
        };
        let value = value.trim();
        if value.is_empty()
            || value == primary_encryption_key
            || keys.iter().any(|existing| existing == value)
        {
            continue;
        }
        keys.push(value.to_string());
    }
    keys
}

fn normalize_string_list(
    raw: Option<serde_json::Value>,
    field_name: &str,
) -> Result<Option<Vec<String>>, DataLayerError> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    normalize_string_list_value(&raw, field_name)
}

fn normalize_string_list_value(
    raw: &serde_json::Value,
    field_name: &str,
) -> Result<Option<Vec<String>>, DataLayerError> {
    match raw {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::Array(items) => normalize_string_list_array(items, field_name).map(Some),
        serde_json::Value::String(raw) => normalize_embedded_string_list(raw, field_name),
        _ => Err(DataLayerError::UnexpectedValue(format!(
            "{field_name} is not a JSON array"
        ))),
    }
}

fn normalize_embedded_string_list(
    raw: &str,
    field_name: &str,
) -> Result<Option<Vec<String>>, DataLayerError> {
    let raw = raw.trim();
    if raw.is_empty() || raw.eq_ignore_ascii_case("null") {
        return Ok(None);
    }
    if let Ok(decoded) = serde_json::from_str::<serde_json::Value>(raw) {
        return normalize_string_list_value(&decoded, field_name);
    }
    Ok(Some(vec![raw.to_string()]))
}

fn normalize_string_list_array(
    items: &[serde_json::Value],
    field_name: &str,
) -> Result<Vec<String>, DataLayerError> {
    let mut values = Vec::with_capacity(items.len());
    for item in items {
        let Some(value) = item.as_str() else {
            return Err(DataLayerError::UnexpectedValue(format!(
                "{field_name} contains a non-string item"
            )));
        };
        let value = value.trim();
        if !value.is_empty() {
            values.push(value.to_string());
        }
    }
    Ok(values)
}
