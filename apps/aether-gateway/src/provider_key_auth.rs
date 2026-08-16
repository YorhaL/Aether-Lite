use aether_data_contracts::repository::provider_catalog::{
    StoredProviderCatalogEndpoint, StoredProviderCatalogKey,
};
use std::collections::BTreeSet;

pub(crate) fn provider_key_configured_api_formats(key: &StoredProviderCatalogKey) -> Vec<String> {
    let mut seen = BTreeSet::new();
    key.api_formats
        .as_ref()
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(crate::ai_serving::normalize_api_format_alias)
                .filter(|value| seen.insert(value.clone()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

pub(crate) fn provider_active_api_formats(
    endpoints: &[StoredProviderCatalogEndpoint],
) -> Vec<String> {
    let mut formats = Vec::new();
    let mut seen = BTreeSet::new();
    for endpoint in endpoints.iter().filter(|endpoint| endpoint.is_active) {
        let api_format = crate::ai_serving::normalize_api_format_alias(&endpoint.api_format);
        if api_format.is_empty() || !seen.insert(api_format.clone()) {
            continue;
        }
        formats.push(api_format);
    }
    formats
}

pub(crate) fn provider_key_effective_api_formats(key: &StoredProviderCatalogKey) -> Vec<String> {
    provider_key_configured_api_formats(key)
}
