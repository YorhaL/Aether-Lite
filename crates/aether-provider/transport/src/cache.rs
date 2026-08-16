use super::snapshot::GatewayProviderTransportSnapshot;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProviderTransportSnapshotCacheKey {
    provider_id: String,
    endpoint_id: String,
    key_id: String,
}

impl ProviderTransportSnapshotCacheKey {
    pub fn new(provider_id: &str, endpoint_id: &str, key_id: &str) -> Option<Self> {
        let provider_id = provider_id.trim();
        let endpoint_id = endpoint_id.trim();
        let key_id = key_id.trim();
        if provider_id.is_empty() || endpoint_id.is_empty() || key_id.is_empty() {
            return None;
        }
        Some(Self {
            provider_id: provider_id.to_string(),
            endpoint_id: endpoint_id.to_string(),
            key_id: key_id.to_string(),
        })
    }
}

pub fn provider_transport_snapshot_looks_refreshed(
    current: &GatewayProviderTransportSnapshot,
    refreshed: &GatewayProviderTransportSnapshot,
) -> bool {
    current.key.decrypted_api_key != refreshed.key.decrypted_api_key
        || current.key.expires_at_unix_secs != refreshed.key.expires_at_unix_secs
}
