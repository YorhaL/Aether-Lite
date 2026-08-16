use super::payload::{
    provider_query_extract_api_key_id, provider_query_extract_api_key_ids,
    provider_query_extract_force_refresh, provider_query_extract_provider_id,
};
use super::response::{
    build_admin_provider_query_bad_request_response, build_admin_provider_query_not_found_response,
    ADMIN_PROVIDER_QUERY_API_KEY_NOT_FOUND_DETAIL, ADMIN_PROVIDER_QUERY_NO_ACTIVE_API_KEY_DETAIL,
    ADMIN_PROVIDER_QUERY_PROVIDER_ID_REQUIRED_DETAIL,
    ADMIN_PROVIDER_QUERY_PROVIDER_NOT_FOUND_DETAIL,
};
use crate::handlers::admin::request::AdminAppState;
use crate::model_fetch::ModelFetchRuntimeState;
use crate::{AppState, GatewayError};
use aether_data_contracts::repository::provider_catalog::{
    StoredProviderCatalogEndpoint, StoredProviderCatalogKey, StoredProviderCatalogProvider,
};
use aether_model_fetch::{
    aggregate_models_for_cache, fetch_models_from_transports, selected_models_fetch_endpoints,
};
use axum::{
    body::Body,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use std::collections::BTreeSet;

const NO_ACTIVE_ENDPOINT_DETAIL: &str = "No active endpoints found for this provider";
const NO_MODELS_FROM_ENDPOINT_DETAIL: &str = "No models returned from any endpoint";
const NO_MODELS_FROM_KEY_DETAIL: &str = "No models returned from any key";

#[derive(Debug)]
struct ProviderQueryKeyFetchResult {
    models: Vec<Value>,
    error: Option<String>,
    warning: Option<String>,
    from_cache: bool,
}

fn select_model_keys(
    keys: Vec<StoredProviderCatalogKey>,
    selected_key_ids: Option<&BTreeSet<String>>,
) -> Result<Vec<StoredProviderCatalogKey>, ()> {
    if selected_key_ids.is_some_and(|selected| {
        selected
            .iter()
            .any(|key_id| !keys.iter().any(|key| key.id == *key_id))
    }) {
        return Err(());
    }
    Ok(match selected_key_ids {
        Some(selected) => keys
            .into_iter()
            .filter(|key| selected.contains(&key.id))
            .collect(),
        None => keys.into_iter().filter(|key| key.is_active).collect(),
    })
}

fn provider_payload(provider: &StoredProviderCatalogProvider) -> Value {
    json!({
        "id": provider.id,
        "name": provider.name,
        "display_name": provider.name,
    })
}

fn key_display_name(key: &StoredProviderCatalogKey) -> &str {
    let name = key.name.trim();
    if name.is_empty() {
        key.id.as_str()
    } else {
        name
    }
}

async fn read_cached_models(
    state: &AdminAppState<'_>,
    provider_id: &str,
    key_id: &str,
) -> Option<Vec<Value>> {
    let cache_key = format!("upstream_models:{provider_id}:{key_id}");
    let raw = state.runtime_state().kv_get(&cache_key).await.ok()??;
    let parsed = serde_json::from_str::<Vec<Value>>(&raw).ok()?;
    Some(aggregate_models_for_cache(&parsed))
}

async fn fetch_models_for_key(
    state: &AdminAppState<'_>,
    provider: &StoredProviderCatalogProvider,
    endpoints: &[StoredProviderCatalogEndpoint],
    key: &StoredProviderCatalogKey,
    force_refresh: bool,
) -> Result<ProviderQueryKeyFetchResult, GatewayError> {
    if !force_refresh {
        if let Some(models) = read_cached_models(state, &provider.id, &key.id).await {
            return Ok(ProviderQueryKeyFetchResult {
                models,
                error: None,
                warning: None,
                from_cache: true,
            });
        }
    }

    let selected_endpoints = selected_models_fetch_endpoints(endpoints, key);
    if selected_endpoints.is_empty() {
        return Ok(ProviderQueryKeyFetchResult {
            models: Vec::new(),
            error: Some(NO_ACTIVE_ENDPOINT_DETAIL.to_string()),
            warning: None,
            from_cache: false,
        });
    }

    let mut transports = Vec::new();
    let mut errors = Vec::new();
    for endpoint in selected_endpoints {
        match state
            .app()
            .read_provider_transport_snapshot(&provider.id, &endpoint.id, &key.id)
            .await?
        {
            Some(transport) => transports.push(transport),
            None => errors.push(format!(
                "{} transport snapshot unavailable",
                endpoint.api_format.trim()
            )),
        }
    }

    if transports.is_empty() {
        return Ok(ProviderQueryKeyFetchResult {
            models: Vec::new(),
            error: Some(errors.join("; ")),
            warning: None,
            from_cache: false,
        });
    }

    let outcome = match fetch_models_from_transports(state.app(), &transports).await {
        Ok(outcome) => outcome,
        Err(error) => {
            errors.push(error);
            return Ok(ProviderQueryKeyFetchResult {
                models: Vec::new(),
                error: Some(errors.join("; ")),
                warning: None,
                from_cache: false,
            });
        }
    };
    errors.extend(outcome.errors);
    let models = aggregate_models_for_cache(&outcome.cached_models);
    if outcome.has_success && !models.is_empty() {
        <AppState as ModelFetchRuntimeState>::write_upstream_models_cache(
            state.app(),
            &provider.id,
            &key.id,
            &models,
        )
        .await;
    }

    let error = if models.is_empty() {
        Some(if errors.is_empty() {
            NO_MODELS_FROM_ENDPOINT_DETAIL.to_string()
        } else {
            errors.join("; ")
        })
    } else {
        None
    };
    let warning = (!models.is_empty() && !errors.is_empty()).then(|| errors.join("; "));
    Ok(ProviderQueryKeyFetchResult {
        models,
        error,
        warning,
        from_cache: false,
    })
}

pub(crate) async fn build_admin_provider_query_models_response(
    state: &AdminAppState<'_>,
    payload: &Value,
) -> Result<Response<Body>, GatewayError> {
    let Some(provider_id) = provider_query_extract_provider_id(payload) else {
        return Ok(build_admin_provider_query_bad_request_response(
            ADMIN_PROVIDER_QUERY_PROVIDER_ID_REQUIRED_DETAIL,
        ));
    };
    let Some(provider) = state
        .app()
        .read_provider_catalog_providers_by_ids(std::slice::from_ref(&provider_id))
        .await?
        .into_iter()
        .find(|provider| provider.id == provider_id)
    else {
        return Ok(build_admin_provider_query_not_found_response(
            ADMIN_PROVIDER_QUERY_PROVIDER_NOT_FOUND_DETAIL,
        ));
    };

    let provider_ids = vec![provider.id.clone()];
    let endpoints = state
        .app()
        .list_provider_catalog_endpoints_by_provider_ids(&provider_ids)
        .await?;
    let keys = state
        .app()
        .list_provider_catalog_keys_by_provider_ids(&provider_ids)
        .await?;
    let force_refresh = provider_query_extract_force_refresh(payload);

    if let Some(key_id) = provider_query_extract_api_key_id(payload) {
        let Some(key) = keys.iter().find(|key| key.id == key_id) else {
            return Ok(build_admin_provider_query_not_found_response(
                ADMIN_PROVIDER_QUERY_API_KEY_NOT_FOUND_DETAIL,
            ));
        };
        let result = fetch_models_for_key(state, &provider, &endpoints, key, force_refresh).await?;
        return Ok(Json(json!({
            "success": !result.models.is_empty(),
            "data": {
                "models": result.models,
                "error": result.error,
                "warning": result.warning,
                "from_cache": result.from_cache,
            },
            "provider": provider_payload(&provider),
        }))
        .into_response());
    }

    let selected_key_ids = provider_query_extract_api_key_ids(payload);
    let query_keys = match select_model_keys(keys, selected_key_ids.as_ref()) {
        Ok(keys) => keys,
        Err(()) => {
            return Ok(build_admin_provider_query_not_found_response(
                ADMIN_PROVIDER_QUERY_API_KEY_NOT_FOUND_DETAIL,
            ));
        }
    };
    if query_keys.is_empty() {
        return Ok(build_admin_provider_query_bad_request_response(
            ADMIN_PROVIDER_QUERY_NO_ACTIVE_API_KEY_DETAIL,
        ));
    }

    let key_count = query_keys.len();
    let mut models = Vec::new();
    let mut issues = Vec::new();
    let mut cache_hits = 0usize;
    for key in &query_keys {
        let result = fetch_models_for_key(state, &provider, &endpoints, key, force_refresh).await?;
        models.extend(result.models);
        if let Some(error) = result.error {
            issues.push(format!("Key {}: {error}", key_display_name(key)));
        }
        if let Some(warning) = result.warning {
            issues.push(format!("Key {}: {warning}", key_display_name(key)));
        }
        if result.from_cache {
            cache_hits += 1;
        }
    }
    let models = aggregate_models_for_cache(&models);
    let success = !models.is_empty();
    let error = (!success).then(|| {
        if issues.is_empty() {
            NO_MODELS_FROM_KEY_DETAIL.to_string()
        } else {
            issues.join("; ")
        }
    });
    let warning = (success && !issues.is_empty()).then(|| issues.join("; "));

    Ok(Json(json!({
        "success": success,
        "data": {
            "models": models,
            "error": error,
            "warning": warning,
            "from_cache": cache_hits == key_count,
            "keys_total": key_count,
            "keys_cached": cache_hits,
            "keys_fetched": key_count - cache_hits,
        },
        "provider": provider_payload(&provider),
    }))
    .into_response())
}

#[cfg(test)]
mod tests {
    use super::select_model_keys;
    use aether_data_contracts::repository::provider_catalog::StoredProviderCatalogKey;
    use std::collections::BTreeSet;

    fn key(id: &str, active: bool) -> StoredProviderCatalogKey {
        StoredProviderCatalogKey::new(
            id.to_string(),
            "provider-1".to_string(),
            id.to_string(),
            "api_key".to_string(),
            None,
            active,
        )
        .expect("key")
    }

    #[test]
    fn explicit_key_selection_is_respected() {
        let selected = BTreeSet::from(["key-a".to_string()]);
        let keys = select_model_keys(
            vec![key("key-a", false), key("key-b", true)],
            Some(&selected),
        )
        .expect("selection");
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].id, "key-a");
    }
}
