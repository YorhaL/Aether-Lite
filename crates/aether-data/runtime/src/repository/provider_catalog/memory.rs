use std::collections::BTreeMap;
use std::sync::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use serde_json::{Map, Value};

use super::{
    ProviderCatalogKeyAdaptiveState, ProviderCatalogKeyAdaptiveStateUpdate,
    ProviderCatalogKeyHealthStateUpdate, ProviderCatalogKeyListQuery,
    ProviderCatalogKeyRuntimeMetadataUpdate, ProviderCatalogKeyStatusSnapshotUpdate,
    ProviderCatalogReadRepository, ProviderCatalogSnapshot,
    ProviderCatalogUpstreamMetadataNamespaceUpdate, ProviderCatalogWriteRepository,
    StoredProviderCatalogEndpoint, StoredProviderCatalogKey,
    StoredProviderCatalogKeyMaintenanceSummary, StoredProviderCatalogKeyPage,
    StoredProviderCatalogKeyStats, StoredProviderCatalogProvider,
};
use crate::repository::usage::{ProviderApiKeyUsageContribution, ProviderApiKeyUsageDelta};
use crate::DataLayerError;

#[derive(Debug, Default)]
struct MemoryProviderCatalogIndex {
    providers: BTreeMap<String, StoredProviderCatalogProvider>,
    endpoints: BTreeMap<String, StoredProviderCatalogEndpoint>,
    keys: BTreeMap<String, StoredProviderCatalogKey>,
}

#[derive(Debug, Default)]
pub struct InMemoryProviderCatalogReadRepository {
    index: RwLock<MemoryProviderCatalogIndex>,
}

impl InMemoryProviderCatalogReadRepository {
    pub fn seed(
        providers: Vec<StoredProviderCatalogProvider>,
        endpoints: Vec<StoredProviderCatalogEndpoint>,
        keys: Vec<StoredProviderCatalogKey>,
    ) -> Self {
        Self {
            index: RwLock::new(MemoryProviderCatalogIndex {
                providers: providers
                    .into_iter()
                    .map(|provider| (provider.id.clone(), provider))
                    .collect(),
                endpoints: endpoints
                    .into_iter()
                    .map(|endpoint| (endpoint.id.clone(), endpoint))
                    .collect(),
                keys: keys.into_iter().map(|key| (key.id.clone(), key)).collect(),
            }),
        }
    }

    fn snapshot(&self) -> ProviderCatalogSnapshot {
        let index = self.index.read().expect("provider catalog repository lock");
        ProviderCatalogSnapshot::new(
            index.providers.values().cloned().collect(),
            index.endpoints.values().cloned().collect(),
            index.keys.values().cloned().collect(),
        )
    }

    pub(crate) fn apply_usage_stats_delta(
        &self,
        key_id: &str,
        delta: &ProviderApiKeyUsageDelta,
        recomputed_last_used_at_unix_secs: Option<u64>,
    ) {
        let mut index = self
            .index
            .write()
            .expect("provider catalog repository lock");
        let Some(key) = index.keys.get_mut(key_id) else {
            return;
        };

        key.request_count = Some(apply_i64_delta_to_u32(
            key.request_count.unwrap_or_default(),
            delta.request_count,
        ));
        key.success_count = Some(apply_i64_delta_to_u32(
            key.success_count.unwrap_or_default(),
            delta.success_count,
        ));
        key.error_count = Some(apply_i64_delta_to_u32(
            key.error_count.unwrap_or_default(),
            delta.error_count,
        ));
        key.total_tokens = apply_i64_delta_to_u64(key.total_tokens, delta.total_tokens);
        key.total_cost_usd = apply_f64_delta(key.total_cost_usd, delta.total_cost_usd);
        key.total_response_time_ms = Some(apply_i64_delta_to_u64(
            key.total_response_time_ms.unwrap_or_default(),
            delta.total_response_time_ms,
        ));

        if let Some(candidate) = delta.candidate_last_used_at_unix_secs {
            key.last_used_at_unix_secs = Some(
                key.last_used_at_unix_secs
                    .map(|existing| existing.max(candidate))
                    .unwrap_or(candidate),
            );
        } else if delta.removed_last_used_at_unix_secs.is_some()
            && key.last_used_at_unix_secs == delta.removed_last_used_at_unix_secs
        {
            key.last_used_at_unix_secs = recomputed_last_used_at_unix_secs;
        }
    }

    pub(crate) fn rebuild_usage_stats(
        &self,
        contributions: &BTreeMap<String, ProviderApiKeyUsageContribution>,
    ) {
        let mut index = self
            .index
            .write()
            .expect("provider catalog repository lock");
        for key in index.keys.values_mut() {
            key.request_count = Some(0);
            key.success_count = Some(0);
            key.error_count = Some(0);
            key.total_tokens = 0;
            key.total_cost_usd = 0.0;
            key.total_response_time_ms = Some(0);
            key.last_used_at_unix_secs = None;
        }

        for (key_id, contribution) in contributions {
            let Some(key) = index.keys.get_mut(key_id) else {
                continue;
            };
            key.request_count = Some(clamp_i64_to_u32(contribution.request_count));
            key.success_count = Some(clamp_i64_to_u32(contribution.success_count));
            key.error_count = Some(clamp_i64_to_u32(contribution.error_count));
            key.total_tokens = clamp_i64_to_u64(contribution.total_tokens);
            key.total_cost_usd = contribution.total_cost_usd.max(0.0);
            key.total_response_time_ms =
                Some(clamp_i64_to_u64(contribution.total_response_time_ms));
            key.last_used_at_unix_secs = contribution.last_used_at_unix_secs;
        }
    }
}

fn apply_i64_delta_to_u32(current: u32, delta: i64) -> u32 {
    clamp_i64_to_u32(i64::from(current).saturating_add(delta))
}

fn apply_i64_delta_to_u64(current: u64, delta: i64) -> u64 {
    if delta >= 0 {
        current.saturating_add(delta as u64)
    } else {
        current.saturating_sub(delta.unsigned_abs())
    }
}

fn clamp_i64_to_u32(value: i64) -> u32 {
    value.clamp(0, i64::from(u32::MAX)) as u32
}

fn clamp_i64_to_u64(value: i64) -> u64 {
    value.max(0) as u64
}

fn apply_f64_delta(current: f64, delta: f64) -> f64 {
    if !current.is_finite() && !delta.is_finite() {
        return 0.0;
    }
    let next = current.max(0.0) + delta;
    if next.is_finite() {
        next.max(0.0)
    } else {
        0.0
    }
}

#[async_trait]
impl ProviderCatalogReadRepository for InMemoryProviderCatalogReadRepository {
    async fn list_providers(
        &self,
        active_only: bool,
    ) -> Result<Vec<StoredProviderCatalogProvider>, DataLayerError> {
        Ok(self.snapshot().list_providers(active_only))
    }

    async fn list_providers_by_ids(
        &self,
        provider_ids: &[String],
    ) -> Result<Vec<StoredProviderCatalogProvider>, DataLayerError> {
        Ok(self.snapshot().list_providers_by_ids(provider_ids))
    }

    async fn list_endpoints_by_ids(
        &self,
        endpoint_ids: &[String],
    ) -> Result<Vec<StoredProviderCatalogEndpoint>, DataLayerError> {
        Ok(self.snapshot().list_endpoints_by_ids(endpoint_ids))
    }

    async fn list_endpoints_by_provider_ids(
        &self,
        provider_ids: &[String],
    ) -> Result<Vec<StoredProviderCatalogEndpoint>, DataLayerError> {
        Ok(self.snapshot().list_endpoints_by_provider_ids(provider_ids))
    }

    async fn list_keys_by_ids(
        &self,
        key_ids: &[String],
    ) -> Result<Vec<StoredProviderCatalogKey>, DataLayerError> {
        Ok(self.snapshot().list_keys_by_ids(key_ids))
    }

    async fn list_keys_by_provider_ids(
        &self,
        provider_ids: &[String],
    ) -> Result<Vec<StoredProviderCatalogKey>, DataLayerError> {
        Ok(self.snapshot().list_keys_by_provider_ids(provider_ids))
    }

    async fn list_key_summaries_by_provider_ids(
        &self,
        provider_ids: &[String],
    ) -> Result<Vec<StoredProviderCatalogKey>, DataLayerError> {
        Ok(self.snapshot().list_keys_by_provider_ids(provider_ids))
    }

    async fn list_key_maintenance_summaries_by_provider_ids(
        &self,
        provider_ids: &[String],
    ) -> Result<Vec<StoredProviderCatalogKeyMaintenanceSummary>, DataLayerError> {
        Ok(self
            .snapshot()
            .list_key_maintenance_summaries_by_provider_ids(provider_ids))
    }

    async fn list_keys_page(
        &self,
        query: &ProviderCatalogKeyListQuery,
    ) -> Result<StoredProviderCatalogKeyPage, DataLayerError> {
        Ok(self.snapshot().list_keys_page(query))
    }

    async fn list_key_stats_by_provider_ids(
        &self,
        provider_ids: &[String],
    ) -> Result<Vec<StoredProviderCatalogKeyStats>, DataLayerError> {
        self.snapshot().list_key_stats_by_provider_ids(provider_ids)
    }
}

#[async_trait]
impl ProviderCatalogWriteRepository for InMemoryProviderCatalogReadRepository {
    async fn create_provider(
        &self,
        provider: &StoredProviderCatalogProvider,
        shift_existing_priorities_from: Option<i32>,
    ) -> Result<StoredProviderCatalogProvider, DataLayerError> {
        let mut index = self
            .index
            .write()
            .expect("provider catalog repository lock");
        if let Some(target_priority) = shift_existing_priorities_from {
            for existing in index.providers.values_mut() {
                if existing.provider_priority >= target_priority {
                    existing.provider_priority += 1;
                }
            }
        }
        index
            .providers
            .insert(provider.id.clone(), provider.clone());
        Ok(provider.clone())
    }

    async fn update_provider(
        &self,
        provider: &StoredProviderCatalogProvider,
    ) -> Result<StoredProviderCatalogProvider, DataLayerError> {
        let mut index = self
            .index
            .write()
            .expect("provider catalog repository lock");
        let Some(stored) = index.providers.get_mut(&provider.id) else {
            return Err(DataLayerError::UnexpectedValue(format!(
                "provider catalog provider {} not found",
                provider.id
            )));
        };
        *stored = provider.clone();
        Ok(stored.clone())
    }

    async fn delete_provider(&self, provider_id: &str) -> Result<bool, DataLayerError> {
        let mut index = self
            .index
            .write()
            .expect("provider catalog repository lock");
        Ok(index.providers.remove(provider_id).is_some())
    }

    async fn cleanup_deleted_provider_refs(
        &self,
        _provider_id: &str,
        _provider_deleted: bool,
        _endpoint_ids: &[String],
        _key_ids: &[String],
    ) -> Result<(), DataLayerError> {
        Ok(())
    }

    async fn create_endpoint(
        &self,
        endpoint: &StoredProviderCatalogEndpoint,
    ) -> Result<StoredProviderCatalogEndpoint, DataLayerError> {
        let mut index = self
            .index
            .write()
            .expect("provider catalog repository lock");
        index
            .endpoints
            .insert(endpoint.id.clone(), endpoint.clone());
        Ok(endpoint.clone())
    }

    async fn update_endpoint(
        &self,
        endpoint: &StoredProviderCatalogEndpoint,
    ) -> Result<StoredProviderCatalogEndpoint, DataLayerError> {
        let mut index = self
            .index
            .write()
            .expect("provider catalog repository lock");
        let Some(stored) = index.endpoints.get_mut(&endpoint.id) else {
            return Err(DataLayerError::UnexpectedValue(format!(
                "provider catalog endpoint {} not found",
                endpoint.id
            )));
        };
        *stored = endpoint.clone();
        Ok(stored.clone())
    }

    async fn delete_endpoint(&self, endpoint_id: &str) -> Result<bool, DataLayerError> {
        let mut index = self
            .index
            .write()
            .expect("provider catalog repository lock");
        Ok(index.endpoints.remove(endpoint_id).is_some())
    }

    async fn create_key(
        &self,
        key: &StoredProviderCatalogKey,
    ) -> Result<StoredProviderCatalogKey, DataLayerError> {
        let mut index = self
            .index
            .write()
            .expect("provider catalog repository lock");
        index.keys.insert(key.id.clone(), key.clone());
        Ok(key.clone())
    }

    async fn update_key(
        &self,
        key: &StoredProviderCatalogKey,
    ) -> Result<StoredProviderCatalogKey, DataLayerError> {
        let mut index = self
            .index
            .write()
            .expect("provider catalog repository lock");
        let Some(stored) = index.keys.get_mut(&key.id) else {
            return Err(DataLayerError::UnexpectedValue(format!(
                "provider catalog key {} not found",
                key.id
            )));
        };
        *stored = merge_admin_key_update(stored, key);
        Ok(stored.clone())
    }

    async fn update_keys(
        &self,
        keys: &[StoredProviderCatalogKey],
    ) -> Result<Vec<StoredProviderCatalogKey>, DataLayerError> {
        let mut index = self
            .index
            .write()
            .expect("provider catalog repository lock");
        for key in keys {
            if !index.keys.contains_key(&key.id) {
                return Err(DataLayerError::UnexpectedValue(format!(
                    "provider catalog key {} not found",
                    key.id
                )));
            }
        }
        for key in keys {
            let stored = index
                .keys
                .get_mut(&key.id)
                .expect("provider catalog key existence was validated");
            *stored = merge_admin_key_update(stored, key);
        }
        Ok(keys
            .iter()
            .filter_map(|key| index.keys.get(&key.id).cloned())
            .collect())
    }

    async fn update_key_upstream_metadata(
        &self,
        key_id: &str,
        upstream_metadata: Option<&serde_json::Value>,
        updated_at_unix_secs: Option<u64>,
    ) -> Result<bool, DataLayerError> {
        let mut index = self
            .index
            .write()
            .expect("provider catalog repository lock");
        let Some(key) = index.keys.get_mut(key_id) else {
            return Ok(false);
        };

        key.upstream_metadata = upstream_metadata.cloned();
        key.updated_at_unix_secs = Some(updated_at_unix_secs.unwrap_or_else(current_unix_secs));
        Ok(true)
    }

    async fn upsert_key_upstream_metadata_namespace(
        &self,
        key_id: &str,
        namespace: &str,
        value: &serde_json::Value,
        updated_at_unix_secs: Option<u64>,
    ) -> Result<bool, DataLayerError> {
        if namespace.trim().is_empty() {
            return Err(DataLayerError::InvalidInput(
                "provider catalog upstream metadata namespace is empty".to_string(),
            ));
        }
        let mut index = self
            .index
            .write()
            .expect("provider catalog repository lock");
        let Some(key) = index.keys.get_mut(key_id) else {
            return Ok(false);
        };
        let metadata = key
            .upstream_metadata
            .get_or_insert_with(|| serde_json::json!({}));
        let Some(metadata) = metadata.as_object_mut() else {
            return Err(DataLayerError::UnexpectedValue(
                "provider catalog upstream metadata must be an object".to_string(),
            ));
        };
        metadata.insert(namespace.to_string(), value.clone());
        key.updated_at_unix_secs = Some(updated_at_unix_secs.unwrap_or_else(current_unix_secs));
        Ok(true)
    }

    async fn update_key_model_fetch_state(
        &self,
        key_id: &str,
        allowed_models: Option<&serde_json::Value>,
        last_models_fetch_at_unix_secs: Option<u64>,
        last_models_fetch_error: Option<&str>,
        updated_at_unix_secs: Option<u64>,
    ) -> Result<bool, DataLayerError> {
        let mut index = self
            .index
            .write()
            .expect("provider catalog repository lock");
        let Some(key) = index.keys.get_mut(key_id) else {
            return Ok(false);
        };
        key.allowed_models = allowed_models.cloned();
        key.last_models_fetch_at_unix_secs = last_models_fetch_at_unix_secs;
        key.last_models_fetch_error = last_models_fetch_error.map(str::to_string);
        key.updated_at_unix_secs = Some(updated_at_unix_secs.unwrap_or_else(current_unix_secs));
        Ok(true)
    }

    async fn update_key_model_fetch_success(
        &self,
        key_id: &str,
        allowed_models: Option<&serde_json::Value>,
        last_models_fetch_at_unix_secs: u64,
        upstream_metadata_updates: &[ProviderCatalogUpstreamMetadataNamespaceUpdate],
        updated_at_unix_secs: Option<u64>,
    ) -> Result<bool, DataLayerError> {
        if upstream_metadata_updates
            .iter()
            .any(|update| update.namespace.trim().is_empty())
        {
            return Err(DataLayerError::InvalidInput(
                "provider catalog upstream metadata namespace is empty".to_string(),
            ));
        }
        let mut index = self
            .index
            .write()
            .expect("provider catalog repository lock");
        let Some(key) = index.keys.get_mut(key_id) else {
            return Ok(false);
        };
        if !upstream_metadata_updates.is_empty()
            && key
                .upstream_metadata
                .as_ref()
                .is_some_and(|metadata| !metadata.is_object())
        {
            return Err(DataLayerError::UnexpectedValue(
                "provider catalog upstream metadata must be an object".to_string(),
            ));
        }

        key.allowed_models = allowed_models.cloned();
        key.last_models_fetch_at_unix_secs = Some(last_models_fetch_at_unix_secs);
        key.last_models_fetch_error = None;
        key.updated_at_unix_secs = Some(updated_at_unix_secs.unwrap_or_else(current_unix_secs));
        if !upstream_metadata_updates.is_empty() {
            let metadata = key
                .upstream_metadata
                .get_or_insert_with(|| serde_json::json!({}))
                .as_object_mut()
                .expect("upstream metadata object was validated");
            for update in upstream_metadata_updates {
                metadata.insert(update.namespace.clone(), update.value.clone());
            }
        }
        Ok(true)
    }

    async fn delete_key(&self, key_id: &str) -> Result<bool, DataLayerError> {
        let mut index = self
            .index
            .write()
            .expect("provider catalog repository lock");
        Ok(index.keys.remove(key_id).is_some())
    }

    async fn update_key_health_state(
        &self,
        key_id: &str,
        is_active: bool,
        health_by_format: Option<&serde_json::Value>,
        circuit_breaker_by_format: Option<&serde_json::Value>,
    ) -> Result<bool, DataLayerError> {
        let mut index = self
            .index
            .write()
            .expect("provider catalog repository lock");
        let Some(key) = index.keys.get_mut(key_id) else {
            return Ok(false);
        };

        key.is_active = is_active;
        key.health_by_format = health_by_format.cloned();
        key.circuit_breaker_by_format = circuit_breaker_by_format.cloned();
        key.updated_at_unix_secs = Some(current_unix_secs());
        Ok(true)
    }

    async fn reset_key_error_count(&self, key_id: &str) -> Result<bool, DataLayerError> {
        let mut index = self
            .index
            .write()
            .expect("provider catalog repository lock");
        let Some(key) = index.keys.get_mut(key_id) else {
            return Ok(false);
        };

        key.error_count = Some(0);
        key.updated_at_unix_secs = Some(current_unix_secs());
        Ok(true)
    }

    async fn compare_and_update_key_adaptive_state(
        &self,
        update: &ProviderCatalogKeyAdaptiveStateUpdate,
    ) -> Result<bool, DataLayerError> {
        if update.key_id.trim().is_empty() {
            return Err(DataLayerError::InvalidInput(
                "provider catalog key_id is empty".to_string(),
            ));
        }
        let patch = adaptive_status_snapshot_patch(&update.status_snapshot_patch)?;
        let mut index = self
            .index
            .write()
            .expect("provider catalog repository lock");
        let Some(key) = index.keys.get_mut(&update.key_id) else {
            return Ok(false);
        };
        let expected = update.expected.canonicalized();
        let next = update.next.canonicalized();
        if ProviderCatalogKeyAdaptiveState::from(&*key) != expected {
            return Ok(false);
        }
        let status_snapshot = json_object_for_merge(
            key.status_snapshot.as_ref(),
            "provider catalog status snapshot",
        )?;
        key.learned_rpm_limit = next.learned_rpm_limit;
        key.concurrent_429_count = next.concurrent_429_count;
        key.rpm_429_count = next.rpm_429_count;
        key.last_429_at_unix_secs = next.last_429_at_unix_secs;
        key.last_429_type.clone_from(&next.last_429_type);
        key.adjustment_history.clone_from(&next.adjustment_history);
        key.utilization_samples
            .clone_from(&next.utilization_samples);
        key.last_probe_increase_at_unix_secs = next.last_probe_increase_at_unix_secs;
        key.last_rpm_peak = next.last_rpm_peak;
        key.status_snapshot = Some(Value::Object(merge_json_objects(status_snapshot, patch)));
        key.updated_at_unix_secs = Some(
            update
                .updated_at_unix_secs
                .unwrap_or_else(current_unix_secs),
        );
        Ok(true)
    }

    async fn update_key_runtime_metadata(
        &self,
        update: &ProviderCatalogKeyRuntimeMetadataUpdate,
    ) -> Result<bool, DataLayerError> {
        if update.key_id.trim().is_empty() || update.namespace.trim().is_empty() {
            return Err(DataLayerError::InvalidInput(
                "provider catalog key_id and runtime metadata namespace are required".to_string(),
            ));
        }
        let status_patch = update
            .status_snapshot_patch
            .as_object()
            .cloned()
            .ok_or_else(|| {
                DataLayerError::InvalidInput(
                    "provider catalog runtime status snapshot patch must be an object".to_string(),
                )
            })?;
        let mut index = self
            .index
            .write()
            .expect("provider catalog repository lock");
        let Some(key) = index.keys.get_mut(&update.key_id) else {
            return Ok(false);
        };
        if key
            .upstream_metadata
            .as_ref()
            .is_some_and(|metadata| !metadata.is_object())
        {
            return Ok(false);
        }
        let current_namespace = key
            .upstream_metadata
            .as_ref()
            .and_then(Value::as_object)
            .and_then(|metadata| metadata.get(&update.namespace))
            .cloned();
        if current_namespace != update.expected_upstream_metadata_value {
            return Ok(false);
        }
        let mut metadata = json_object_for_merge(
            key.upstream_metadata.as_ref(),
            "provider catalog upstream metadata",
        )?;
        let status_snapshot = json_object_for_merge(
            key.status_snapshot.as_ref(),
            "provider catalog status snapshot",
        )?;
        metadata.insert(
            update.namespace.clone(),
            update.upstream_metadata_value.clone(),
        );
        key.upstream_metadata = Some(Value::Object(metadata));
        key.status_snapshot = Some(Value::Object(merge_json_objects(
            status_snapshot,
            status_patch,
        )));
        key.updated_at_unix_secs = Some(
            update
                .updated_at_unix_secs
                .unwrap_or_else(current_unix_secs),
        );
        Ok(true)
    }

    async fn update_key_status_snapshot(
        &self,
        update: &ProviderCatalogKeyStatusSnapshotUpdate,
    ) -> Result<bool, DataLayerError> {
        if update.key_id.trim().is_empty() {
            return Err(DataLayerError::InvalidInput(
                "provider catalog key_id is empty".to_string(),
            ));
        }
        let patch = update
            .status_snapshot_patch
            .as_object()
            .cloned()
            .ok_or_else(|| {
                DataLayerError::InvalidInput(
                    "provider catalog status snapshot patch must be an object".to_string(),
                )
            })?;
        let mut index = self
            .index
            .write()
            .expect("provider catalog repository lock");
        let Some(key) = index.keys.get_mut(&update.key_id) else {
            return Ok(false);
        };
        let status_snapshot = json_object_for_merge(
            key.status_snapshot.as_ref(),
            "provider catalog status snapshot",
        )?;
        key.status_snapshot = Some(Value::Object(merge_json_objects(status_snapshot, patch)));
        key.updated_at_unix_secs = Some(
            update
                .updated_at_unix_secs
                .unwrap_or_else(current_unix_secs),
        );
        Ok(true)
    }

    async fn compare_and_update_key_health_state(
        &self,
        update: &ProviderCatalogKeyHealthStateUpdate,
    ) -> Result<bool, DataLayerError> {
        if update.key_id.trim().is_empty() {
            return Err(DataLayerError::InvalidInput(
                "provider catalog key_id is empty".to_string(),
            ));
        }
        let mut index = self
            .index
            .write()
            .expect("provider catalog repository lock");
        let Some(key) = index.keys.get_mut(&update.key_id) else {
            return Ok(false);
        };
        if key.health_by_format != update.expected_health_by_format
            || key.circuit_breaker_by_format != update.expected_circuit_breaker_by_format
        {
            return Ok(false);
        }
        key.health_by_format.clone_from(&update.health_by_format);
        key.circuit_breaker_by_format
            .clone_from(&update.circuit_breaker_by_format);
        key.updated_at_unix_secs = Some(current_unix_secs());
        Ok(true)
    }
}

fn current_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn merge_admin_key_update(
    stored: &StoredProviderCatalogKey,
    requested: &StoredProviderCatalogKey,
) -> StoredProviderCatalogKey {
    let mut merged = requested.clone();

    // Catalog edits own configuration, not live observations. Keep operational
    // fields from the current row so stale admin snapshots cannot undo runtime writes.
    merged.learned_rpm_limit = stored.learned_rpm_limit;
    merged.concurrent_429_count = stored.concurrent_429_count;
    merged.rpm_429_count = stored.rpm_429_count;
    merged.last_429_at_unix_secs = stored.last_429_at_unix_secs;
    merged.last_429_type.clone_from(&stored.last_429_type);
    merged
        .adjustment_history
        .clone_from(&stored.adjustment_history);
    merged
        .utilization_samples
        .clone_from(&stored.utilization_samples);
    merged.last_probe_increase_at_unix_secs = stored.last_probe_increase_at_unix_secs;
    merged.last_rpm_peak = stored.last_rpm_peak;
    merged.last_models_fetch_at_unix_secs = stored.last_models_fetch_at_unix_secs;
    merged
        .last_models_fetch_error
        .clone_from(&stored.last_models_fetch_error);
    merged
        .upstream_metadata
        .clone_from(&stored.upstream_metadata);
    merged.status_snapshot.clone_from(&stored.status_snapshot);
    merged.health_by_format.clone_from(&stored.health_by_format);
    merged
        .circuit_breaker_by_format
        .clone_from(&stored.circuit_breaker_by_format);
    merged.request_count = stored.request_count;
    merged.total_tokens = stored.total_tokens;
    merged.total_cost_usd = stored.total_cost_usd;
    merged.success_count = stored.success_count;
    merged.error_count = stored.error_count;
    merged.total_response_time_ms = stored.total_response_time_ms;
    merged.last_used_at_unix_secs = stored.last_used_at_unix_secs;
    merged.created_at_unix_ms = stored.created_at_unix_ms;
    merged
}

fn adaptive_status_snapshot_patch(patch: &Value) -> Result<Map<String, Value>, DataLayerError> {
    const OWNED_FIELDS: [&str; 6] = [
        "observation_count",
        "header_observation_count",
        "latest_upstream_limit",
        "learning_confidence",
        "enforcement_active",
        "known_boundary",
    ];
    let object = patch.as_object().ok_or_else(|| {
        DataLayerError::InvalidInput(
            "provider catalog adaptive status snapshot patch must be an object".to_string(),
        )
    })?;
    Ok(OWNED_FIELDS
        .into_iter()
        .filter_map(|field| {
            object
                .get(field)
                .cloned()
                .map(|value| (field.to_string(), value))
        })
        .collect())
}

fn json_object_for_merge(
    value: Option<&Value>,
    field_name: &str,
) -> Result<Map<String, Value>, DataLayerError> {
    match value {
        None => Ok(Map::new()),
        Some(Value::Object(object)) => Ok(object.clone()),
        Some(_) => Err(DataLayerError::UnexpectedValue(format!(
            "{field_name} must be an object"
        ))),
    }
}

fn merge_json_objects(
    mut current: Map<String, Value>,
    patch: Map<String, Value>,
) -> Map<String, Value> {
    current.extend(patch);
    current
}
