use std::collections::{BTreeSet, HashMap};
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use aether_data_contracts::repository::provider_catalog::{
    StoredProviderCatalogEndpoint, StoredProviderCatalogKey, StoredProviderCatalogProvider,
};
use aether_model_fetch::{
    apply_model_filters, fetch_models_from_transports, json_string_list,
    model_fetch_interval_minutes, model_fetch_startup_delay_seconds, model_fetch_startup_enabled,
    selected_models_fetch_endpoints, sync_provider_model_whitelist_associations,
    ModelFetchAssociationStore, ModelFetchRunSummary,
};
use serde_json::{json, Value};
use tracing::{debug, info, warn};

use crate::{AppState, GatewayError};

pub(crate) mod state;

use self::state::ModelFetchRuntimeState;

#[derive(Debug, Clone)]
struct SelectedFetchTarget {
    provider: StoredProviderCatalogProvider,
    key: StoredProviderCatalogKey,
    endpoints: Vec<StoredProviderCatalogEndpoint>,
}

pub(crate) fn spawn_model_fetch_worker(state: AppState) -> Option<tokio::task::JoinHandle<()>> {
    if !state.has_provider_catalog_data_reader() || !state.has_provider_catalog_data_writer() {
        return None;
    }

    Some(crate::task_runtime::spawn_singleton_worker(
        state,
        crate::task_runtime::TASK_KEY_MODEL_FETCH_WORKER,
        |state| async move {
            if model_fetch_startup_enabled() {
                let startup_delay = model_fetch_startup_delay_seconds();
                if startup_delay > 0 {
                    tokio::time::sleep(Duration::from_secs(startup_delay)).await;
                }
                if let Err(err) = run_model_fetch_cycle(&state, "startup").await {
                    warn!(error = ?err, "gateway model fetch startup failed");
                }
            } else {
                info!("gateway model fetch startup disabled");
            }

            let mut interval = tokio::time::interval(Duration::from_secs(
                model_fetch_interval_minutes().saturating_mul(60),
            ));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            interval.tick().await;
            loop {
                interval.tick().await;
                if let Err(err) = run_model_fetch_cycle(&state, "tick").await {
                    warn!(error = ?err, "gateway model fetch tick failed");
                }
            }
        },
    ))
}

pub(crate) async fn perform_model_fetch_once(
    state: &AppState,
) -> Result<ModelFetchRunSummary, GatewayError> {
    perform_model_fetch_once_with_state(state).await
}

pub(crate) async fn perform_model_fetch_for_key(
    state: &AppState,
    provider_id: &str,
    key_id: &str,
) -> Result<ModelFetchRunSummary, GatewayError> {
    let key_ids = BTreeSet::from([key_id.to_string()]);
    perform_model_fetch_for_keys_with_state(state, provider_id, &key_ids).await
}

pub(crate) async fn perform_model_fetch_for_keys(
    state: &AppState,
    provider_id: &str,
    key_ids: &BTreeSet<String>,
) -> Result<ModelFetchRunSummary, GatewayError> {
    perform_model_fetch_for_keys_with_state(state, provider_id, key_ids).await
}

async fn perform_model_fetch_once_with_state<S>(
    state: &S,
) -> Result<ModelFetchRunSummary, GatewayError>
where
    S: ModelFetchRuntimeState + ?Sized,
{
    let targets = collect_fetch_targets(state, None, None).await?;
    execute_fetch_targets(state, targets).await
}

async fn perform_model_fetch_for_keys_with_state<S>(
    state: &S,
    provider_id: &str,
    key_ids: &BTreeSet<String>,
) -> Result<ModelFetchRunSummary, GatewayError>
where
    S: ModelFetchRuntimeState + ?Sized,
{
    let targets = collect_fetch_targets(state, Some(provider_id), Some(key_ids)).await?;
    execute_fetch_targets(state, targets).await
}

async fn collect_fetch_targets<S>(
    state: &S,
    provider_id_filter: Option<&str>,
    key_id_filter: Option<&BTreeSet<String>>,
) -> Result<Vec<SelectedFetchTarget>, GatewayError>
where
    S: ModelFetchRuntimeState + ?Sized,
{
    if !state.has_provider_catalog_data_reader() || !state.has_provider_catalog_data_writer() {
        return Ok(Vec::new());
    }

    let providers = state
        .list_provider_catalog_providers(true)
        .await?
        .into_iter()
        .filter(|provider| provider_id_filter.is_none_or(|provider_id| provider.id == provider_id))
        .collect::<Vec<_>>();
    if providers.is_empty() {
        return Ok(Vec::new());
    }

    let provider_ids = providers
        .iter()
        .map(|provider| provider.id.clone())
        .collect::<Vec<_>>();
    let mut endpoints_by_provider = HashMap::<String, Vec<StoredProviderCatalogEndpoint>>::new();
    for endpoint in state
        .list_provider_catalog_endpoints_by_provider_ids(&provider_ids)
        .await?
    {
        endpoints_by_provider
            .entry(endpoint.provider_id.clone())
            .or_default()
            .push(endpoint);
    }
    let mut keys_by_provider = HashMap::<String, Vec<StoredProviderCatalogKey>>::new();
    for key in <S as ModelFetchAssociationStore>::list_provider_catalog_keys_by_provider_ids(
        state,
        &provider_ids,
    )
    .await
    .map_err(GatewayError::Internal)?
    {
        keys_by_provider
            .entry(key.provider_id.clone())
            .or_default()
            .push(key);
    }

    let mut targets = Vec::new();
    for provider in providers {
        let endpoints = endpoints_by_provider
            .remove(&provider.id)
            .unwrap_or_default();
        let keys = keys_by_provider.remove(&provider.id).unwrap_or_default();
        for key in keys {
            if key_id_filter.is_some_and(|key_ids| !key_ids.contains(&key.id)) {
                continue;
            }
            if !key.is_active || !key.auto_fetch_models {
                continue;
            }
            let selected_endpoints = selected_models_fetch_endpoints(&endpoints, &key);
            targets.push(SelectedFetchTarget {
                provider: provider.clone(),
                key,
                endpoints: selected_endpoints,
            });
        }
    }
    Ok(targets)
}

async fn execute_fetch_targets<S>(
    state: &S,
    targets: Vec<SelectedFetchTarget>,
) -> Result<ModelFetchRunSummary, GatewayError>
where
    S: ModelFetchRuntimeState + ?Sized,
{
    let mut summary = ModelFetchRunSummary {
        attempted: targets.len(),
        succeeded: 0,
        failed: 0,
        skipped: 0,
    };
    for target in targets {
        match fetch_and_persist_key_models(state, &target).await? {
            KeyFetchDisposition::Succeeded => summary.succeeded += 1,
            KeyFetchDisposition::Failed => summary.failed += 1,
            KeyFetchDisposition::Skipped => summary.skipped += 1,
        }
    }
    Ok(summary)
}

async fn run_model_fetch_cycle<S>(state: &S, phase: &'static str) -> Result<(), GatewayError>
where
    S: ModelFetchRuntimeState + ?Sized,
{
    let summary = perform_model_fetch_once_with_state(state).await?;
    if summary.attempted == 0 {
        debug!(phase, "gateway model fetch found no eligible keys");
        return Ok(());
    }

    info!(
        phase,
        attempted = summary.attempted,
        succeeded = summary.succeeded,
        failed = summary.failed,
        skipped = summary.skipped,
        "gateway model fetch cycle completed"
    );
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyFetchDisposition {
    Succeeded,
    Failed,
    Skipped,
}

async fn fetch_and_persist_key_models(
    state: &(impl ModelFetchRuntimeState + ?Sized),
    target: &SelectedFetchTarget,
) -> Result<KeyFetchDisposition, GatewayError> {
    let now_unix_secs = now_unix_secs();
    if target.endpoints.is_empty() {
        persist_key_fetch_failure(
            state,
            &target.key,
            now_unix_secs,
            "No supported endpoint for Rust models fetch".to_string(),
        )
        .await?;
        return Ok(KeyFetchDisposition::Skipped);
    }

    let mut transports = Vec::new();
    for endpoint in &target.endpoints {
        match state
            .read_provider_transport_snapshot(&target.provider.id, &endpoint.id, &target.key.id)
            .await?
        {
            Some(transport) => transports.push(transport),
            None => {
                warn!(
                    provider_id = %target.provider.id,
                    endpoint_id = %endpoint.id,
                    key_id = %target.key.id,
                    "gateway model fetch transport snapshot unavailable"
                );
            }
        }
    }

    if transports.is_empty() {
        persist_key_fetch_failure(
            state,
            &target.key,
            now_unix_secs,
            "Provider transport snapshot unavailable".to_string(),
        )
        .await?;
        return Ok(KeyFetchDisposition::Skipped);
    }

    let result = match fetch_models_from_transports(state, &transports).await {
        Ok(result) => result,
        Err(err) => {
            persist_key_fetch_failure(state, &target.key, now_unix_secs, err.clone()).await?;
            warn!(
                provider_id = %target.provider.id,
                key_id = %target.key.id,
                message = %err,
                "gateway model fetch failed"
            );
            return Ok(KeyFetchDisposition::Failed);
        }
    };

    if !result.has_success {
        let error = if result.errors.is_empty() {
            "Upstream models fetch failed".to_string()
        } else {
            result.errors.join("; ")
        };
        persist_key_fetch_failure(state, &target.key, now_unix_secs, error.clone()).await?;
        warn!(
            provider_id = %target.provider.id,
            key_id = %target.key.id,
            message = %error,
            "gateway model fetch failed"
        );
        return Ok(KeyFetchDisposition::Failed);
    }

    let filtered_models = apply_model_filters(
        &result.fetched_model_ids,
        json_string_list(target.key.locked_models.as_ref()),
        json_string_list(target.key.model_include_patterns.as_ref()),
        json_string_list(target.key.model_exclude_patterns.as_ref()),
    );
    persist_key_fetch_success(state, &target.key, now_unix_secs, &filtered_models).await?;
    state
        .write_upstream_models_cache(&target.provider.id, &target.key.id, &result.cached_models)
        .await;
    sync_provider_model_whitelist_associations(state, &target.provider.id, &filtered_models)
        .await
        .map_err(GatewayError::Internal)?;
    Ok(KeyFetchDisposition::Succeeded)
}

async fn persist_key_fetch_failure(
    state: &(impl ModelFetchRuntimeState + ?Sized),
    key: &StoredProviderCatalogKey,
    now_unix_secs: u64,
    error: String,
) -> Result<(), GatewayError> {
    state
        .update_provider_catalog_key_model_fetch_state(
            &key.id,
            key.allowed_models.as_ref(),
            Some(now_unix_secs),
            Some(&error),
            Some(now_unix_secs),
        )
        .await?;
    Ok(())
}

async fn persist_key_fetch_success(
    state: &(impl ModelFetchRuntimeState + ?Sized),
    key: &StoredProviderCatalogKey,
    now_unix_secs: u64,
    allowed_models: &[String],
) -> Result<(), GatewayError> {
    let allowed_models = if allowed_models.is_empty() {
        None
    } else {
        Some(json!(allowed_models))
    };
    state
        .update_provider_catalog_key_model_fetch_state(
            &key.id,
            allowed_models.as_ref(),
            Some(now_unix_secs),
            None,
            Some(now_unix_secs),
        )
        .await?;
    Ok(())
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
