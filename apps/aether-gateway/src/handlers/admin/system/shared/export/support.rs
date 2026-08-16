use super::super::configs::is_sensitive_admin_system_config_key;
use crate::api::ai::admin_endpoint_signature_parts;
use crate::handlers::admin::provider::write::normalize::retain_supported_provider_config;
use crate::handlers::admin::request::AdminAppState;
use crate::handlers::shared::decrypt_catalog_secret_with_fallbacks;
pub(crate) use aether_admin::system::ADMIN_SYSTEM_CONFIG_EXPORT_VERSION;
use aether_data_contracts::repository::provider_catalog::StoredProviderCatalogEndpoint;

pub(crate) const ADMIN_SYSTEM_EXPORT_PAGE_LIMIT: usize = 10_000;

pub(crate) fn decrypt_admin_system_export_secret(
    state: &AdminAppState<'_>,
    ciphertext: &str,
) -> Option<String> {
    decrypt_catalog_secret_with_fallbacks(state.encryption_key(), ciphertext)
}

pub(super) fn normalize_admin_system_export_api_formats(
    raw_formats: Option<&serde_json::Value>,
) -> Vec<String> {
    aether_admin::system::normalize_admin_system_export_api_formats(raw_formats, |value| {
        admin_endpoint_signature_parts(value).map(|(signature, _, _)| signature.to_string())
    })
}

pub(super) fn resolve_admin_system_export_key_api_formats(
    raw_formats: Option<&serde_json::Value>,
    provider_endpoint_formats: &[String],
) -> Vec<String> {
    aether_admin::system::resolve_admin_system_export_key_api_formats(
        raw_formats,
        provider_endpoint_formats,
        |value| {
            admin_endpoint_signature_parts(value).map(|(signature, _, _)| signature.to_string())
        },
    )
}

pub(super) fn collect_admin_system_export_provider_endpoint_formats(
    endpoints: &[StoredProviderCatalogEndpoint],
) -> Vec<String> {
    aether_admin::system::collect_admin_system_export_provider_endpoint_formats(
        endpoints,
        |value| {
            admin_endpoint_signature_parts(value).map(|(signature, _, _)| signature.to_string())
        },
    )
}

pub(super) fn admin_system_export_provider_config(
    config: Option<&serde_json::Value>,
) -> Option<serde_json::Value> {
    let mut config = config.cloned()?;
    let object = config.as_object_mut()?;
    retain_supported_provider_config(object);
    (!object.is_empty()).then_some(config)
}
