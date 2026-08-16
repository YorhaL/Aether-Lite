use crate::handlers::admin::request::AdminAppState;
use crate::provider_key_auth::provider_key_effective_api_formats;
use aether_data_contracts::repository::provider_catalog::{
    ProviderCatalogKeyListOrder, ProviderCatalogKeyListQuery,
};
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

async fn build_admin_provider_key_items_payload(
    state: &AdminAppState<'_>,
    provider_id: &str,
    skip: usize,
    limit: usize,
) -> Option<(Vec<Value>, usize)> {
    if !state.has_provider_catalog_data_reader() {
        return None;
    }
    let provider = state
        .read_provider_catalog_providers_by_ids(&[provider_id.to_string()])
        .await
        .ok()
        .and_then(|mut providers| providers.drain(..).next())?;
    let key_page = state
        .list_provider_catalog_key_page(&ProviderCatalogKeyListQuery {
            provider_id: provider.id.clone(),
            search: None,
            is_active: None,
            offset: skip,
            limit,
            order: ProviderCatalogKeyListOrder::CreatedAt,
        })
        .await
        .ok()?;
    let endpoints = state
        .list_provider_catalog_endpoints_by_provider_ids(std::slice::from_ref(&provider.id))
        .await
        .ok()
        .unwrap_or_default();
    let now_unix_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let keys = key_page.items;

    let items = keys
        .into_iter()
        .map(|key| {
            let api_formats = provider_key_effective_api_formats(&key);
            state.build_admin_provider_key_response(&key, &api_formats, now_unix_secs)
        })
        .collect();
    Some((items, key_page.total))
}

pub(crate) async fn build_admin_provider_keys_payload(
    state: &AdminAppState<'_>,
    provider_id: &str,
    skip: usize,
    limit: usize,
) -> Option<Value> {
    let (items, _) =
        build_admin_provider_key_items_payload(state, provider_id, skip, limit).await?;
    Some(Value::Array(items))
}

pub(crate) async fn build_admin_provider_keys_page_payload(
    state: &AdminAppState<'_>,
    provider_id: &str,
    page: usize,
    page_size: usize,
) -> Option<Value> {
    let skip = page.saturating_sub(1).saturating_mul(page_size);
    let (items, total) =
        build_admin_provider_key_items_payload(state, provider_id, skip, page_size).await?;
    Some(json!({
        "total": total,
        "page": page,
        "page_size": page_size,
        "keys": items,
    }))
}
