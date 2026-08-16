use super::super::cache_affinity::{
    clear_admin_monitoring_scheduler_affinity_entries,
    delete_admin_monitoring_cache_affinity_raw_keys,
};
use super::super::cache_identity::admin_monitoring_list_export_api_key_records_by_ids;
use super::super::cache_route_helpers::admin_monitoring_cache_affinity_delete_params_from_path;
use super::super::cache_store::list_admin_monitoring_cache_affinity_records_by_affinity_keys;
use crate::handlers::admin::request::{AdminAppState, AdminRequestContext};
use crate::GatewayError;
use aether_admin::observability::monitoring::{
    admin_monitoring_bad_request_response, admin_monitoring_not_found_response,
    build_admin_monitoring_cache_affinity_delete_success_response,
};
use axum::{body::Body, response::Response};

#[derive(Debug, Default)]
struct AdminMonitoringCacheAffinityDeleteFilter {
    client_family: Option<String>,
    session_hash: Option<String>,
}

fn admin_monitoring_cache_affinity_delete_filter_from_query(
    query: Option<&str>,
) -> AdminMonitoringCacheAffinityDeleteFilter {
    let Some(query) = query else {
        return AdminMonitoringCacheAffinityDeleteFilter::default();
    };
    let mut filter = AdminMonitoringCacheAffinityDeleteFilter::default();
    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        match key.as_ref() {
            "client_family" => filter.client_family = Some(value.to_ascii_lowercase()),
            "session_hash" => filter.session_hash = Some(value.to_string()),
            _ => {}
        }
    }
    filter
}

fn admin_monitoring_cache_affinity_matches_delete_filter(
    item: &super::super::cache_types::AdminMonitoringCacheAffinityRecord,
    filter: &AdminMonitoringCacheAffinityDeleteFilter,
) -> bool {
    if let Some(client_family) = filter.client_family.as_deref() {
        if item.client_family.as_deref() != Some(client_family) {
            return false;
        }
    }
    if let Some(session_hash) = filter.session_hash.as_deref() {
        if item.session_hash.as_deref() != Some(session_hash) {
            return false;
        }
    }
    true
}

pub(in super::super) async fn build_admin_monitoring_cache_affinity_delete_response(
    state: &AdminAppState<'_>,
    request_context: &AdminRequestContext<'_>,
) -> Result<Response<Body>, GatewayError> {
    let Some((affinity_key, endpoint_id, model_id, api_format)) =
        admin_monitoring_cache_affinity_delete_params_from_path(&request_context.request_path)
    else {
        return Ok(admin_monitoring_bad_request_response(
            "缺少 affinity_key、endpoint_id、model_id 或 api_format",
        ));
    };

    let target_affinity_keys =
        std::iter::once(affinity_key.clone()).collect::<std::collections::BTreeSet<_>>();
    let delete_filter = admin_monitoring_cache_affinity_delete_filter_from_query(
        request_context.request_query_string.as_deref(),
    );
    let target_affinities =
        list_admin_monitoring_cache_affinity_records_by_affinity_keys(state, &target_affinity_keys)
            .await?
            .into_iter()
            .filter(|item| {
                item.affinity_key == affinity_key
                    && item.endpoint_id.as_deref() == Some(endpoint_id.as_str())
                    && item.model_name == model_id
                    && item.api_format.eq_ignore_ascii_case(&api_format)
                    && admin_monitoring_cache_affinity_matches_delete_filter(item, &delete_filter)
            })
            .collect::<Vec<_>>();
    if target_affinities.is_empty() {
        return Ok(admin_monitoring_not_found_response(
            "未找到指定的缓存亲和性记录",
        ));
    }

    let raw_keys = target_affinities
        .iter()
        .map(|item| item.raw_key.clone())
        .collect::<Vec<_>>();
    let _ = delete_admin_monitoring_cache_affinity_raw_keys(state, &raw_keys).await?;
    clear_admin_monitoring_scheduler_affinity_entries(state, &target_affinities);

    let mut api_key_by_id = admin_monitoring_list_export_api_key_records_by_ids(
        state,
        std::slice::from_ref(&affinity_key),
    )
    .await?;
    let api_key_name = api_key_by_id
        .remove(&affinity_key)
        .and_then(|item| item.into_stored().name)
        .unwrap_or_else(|| affinity_key.chars().take(8).collect::<String>());

    Ok(
        build_admin_monitoring_cache_affinity_delete_success_response(
            affinity_key,
            endpoint_id,
            model_id,
            api_key_name,
        ),
    )
}
