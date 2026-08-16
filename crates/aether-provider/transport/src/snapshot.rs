use aether_data_contracts::repository::provider_catalog::{
    StoredProviderCatalogEndpoint, StoredProviderCatalogKey, StoredProviderCatalogProvider,
};
use aether_data_contracts::DataLayerError;
use async_trait::async_trait;

#[path = "snapshot_mapping.rs"]
mod snapshot_mapping;

use self::snapshot_mapping::{fallback_encryption_keys, map_endpoint, map_key, map_provider};

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct GatewayProviderTransportSnapshot {
    pub provider: GatewayProviderTransportProvider,
    pub endpoint: GatewayProviderTransportEndpoint,
    pub key: GatewayProviderTransportKey,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct GatewayProviderTransportProvider {
    pub id: String,
    pub name: String,
    pub is_active: bool,
    pub concurrent_limit: Option<i32>,
    pub max_retries: Option<i32>,
    pub request_timeout_secs: Option<f64>,
    pub stream_first_byte_timeout_secs: Option<f64>,
    pub config: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct GatewayProviderTransportEndpoint {
    pub id: String,
    pub provider_id: String,
    pub api_format: String,
    pub is_active: bool,
    pub base_url: String,
    pub header_rules: Option<serde_json::Value>,
    pub body_rules: Option<serde_json::Value>,
    pub max_retries: Option<i32>,
    pub custom_path: Option<String>,
    pub config: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct GatewayProviderTransportKey {
    pub id: String,
    pub provider_id: String,
    pub name: String,
    pub auth_type: String,
    pub is_active: bool,
    pub api_formats: Option<Vec<String>>,
    pub allowed_models: Option<Vec<String>>,
    pub capabilities: Option<serde_json::Value>,
    pub rate_multipliers: Option<serde_json::Value>,
    pub global_priority_by_format: Option<serde_json::Value>,
    pub expires_at_unix_secs: Option<u64>,
    pub fingerprint: Option<serde_json::Value>,
    pub decrypted_api_key: String,
}

#[async_trait]
pub trait ProviderTransportSnapshotSource: Send + Sync {
    fn encryption_key(&self) -> Option<&str>;

    async fn list_provider_catalog_providers_by_ids(
        &self,
        ids: &[String],
    ) -> Result<Vec<StoredProviderCatalogProvider>, DataLayerError>;

    async fn list_provider_catalog_endpoints_by_ids(
        &self,
        ids: &[String],
    ) -> Result<Vec<StoredProviderCatalogEndpoint>, DataLayerError>;

    async fn list_provider_catalog_keys_by_ids(
        &self,
        ids: &[String],
    ) -> Result<Vec<StoredProviderCatalogKey>, DataLayerError>;
}

pub async fn read_provider_transport_snapshot(
    state: &dyn ProviderTransportSnapshotSource,
    provider_id: &str,
    endpoint_id: &str,
    key_id: &str,
) -> Result<Option<GatewayProviderTransportSnapshot>, DataLayerError> {
    let Some(encryption_key) = state.encryption_key() else {
        return Ok(None);
    };
    let fallback_encryption_keys = fallback_encryption_keys(encryption_key);
    let provider_ids = [provider_id.to_string()];
    let endpoint_ids = [endpoint_id.to_string()];
    let key_ids = [key_id.to_string()];
    let (providers, endpoints, keys) = tokio::try_join!(
        state.list_provider_catalog_providers_by_ids(&provider_ids),
        state.list_provider_catalog_endpoints_by_ids(&endpoint_ids),
        state.list_provider_catalog_keys_by_ids(&key_ids),
    )?;

    let Some(provider) = providers.into_iter().next() else {
        return Ok(None);
    };
    let Some(endpoint) = endpoints.into_iter().next() else {
        return Ok(None);
    };
    let Some(key) = keys.into_iter().next() else {
        return Ok(None);
    };

    if endpoint.provider_id != provider.id {
        return Err(DataLayerError::UnexpectedValue(format!(
            "provider_endpoints.provider_id mismatch: expected {}, got {}",
            provider.id, endpoint.provider_id
        )));
    }
    if key.provider_id != provider.id {
        return Err(DataLayerError::UnexpectedValue(format!(
            "provider_api_keys.provider_id mismatch: expected {}, got {}",
            provider.id, key.provider_id
        )));
    }

    Ok(Some(GatewayProviderTransportSnapshot {
        provider: map_provider(provider),
        endpoint: map_endpoint(endpoint),
        key: map_key(key, encryption_key, &fallback_encryption_keys)?,
    }))
}
