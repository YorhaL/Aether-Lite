use crate::constants::INTERNAL_GATEWAY_PATH_PREFIXES;
use crate::handlers::admin::request::{AdminAppState, AdminRequestContext};
use crate::handlers::admin::shared::build_admin_usage_counter_health_payload;
use crate::GatewayError;
use aether_admin::observability::monitoring::build_admin_monitoring_system_status_payload_response;
use aether_data_contracts::repository::usage::{
    UsageAuditSummaryQuery, UsageMonitoringErrorCountQuery,
};
use axum::{body::Body, response::Response};

pub(super) async fn build_admin_monitoring_system_status_response(
    state: &AdminAppState<'_>,
) -> Result<Response<Body>, GatewayError> {
    let state = state.as_ref();
    let now = chrono::Utc::now();
    let today_start = now
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .expect("midnight should be valid")
        .and_utc();
    let recent_error_from = now - chrono::Duration::hours(1);
    let now_unix_secs = now.timestamp().max(0) as u64;

    let user_summary = state.summarize_export_users().await?;
    let total_users = user_summary.total;
    let active_users = user_summary.active;

    let providers = state
        .data
        .list_provider_catalog_providers(false)
        .await
        .map_err(|err| GatewayError::Internal(err.to_string()))?;
    let total_providers = providers.len();
    let active_providers = providers.iter().filter(|item| item.is_active).count();

    let user_api_key_summary = state
        .summarize_auth_api_key_export_non_standalone_records(now_unix_secs)
        .await?;
    let standalone_api_key_summary = state
        .summarize_auth_api_key_export_standalone_records(now_unix_secs)
        .await?;
    let total_api_keys = user_api_key_summary
        .total
        .saturating_add(standalone_api_key_summary.total);
    let active_api_keys = user_api_key_summary
        .active
        .saturating_add(standalone_api_key_summary.active);

    let today_usage = state
        .summarize_usage_audits(&UsageAuditSummaryQuery {
            created_from_unix_secs: today_start.timestamp().max(0) as u64,
            created_until_unix_secs: now_unix_secs.saturating_add(1),
            user_id: None,
            provider_name: None,
            model: None,
        })
        .await?;
    let today_requests = usize::try_from(today_usage.total_requests).unwrap_or(usize::MAX);
    let today_tokens = today_usage.recorded_total_tokens;
    let today_cost = today_usage.total_cost_usd;

    let recent_errors = usize::try_from(
        state
            .count_monitoring_usage_errors(&UsageMonitoringErrorCountQuery {
                created_from_unix_secs: recent_error_from.timestamp().max(0) as u64,
                created_until_unix_secs: now_unix_secs.saturating_add(1),
            })
            .await?,
    )
    .unwrap_or(usize::MAX);
    let usage_counter_snapshot = state
        .read_cached_usage_counter_health()
        .await
        .map_err(|err| GatewayError::Internal(err.to_string()))?;
    let usage_counter =
        build_admin_usage_counter_health_payload(&usage_counter_snapshot, now_unix_secs);

    Ok(build_admin_monitoring_system_status_payload_response(
        now,
        total_users,
        active_users,
        total_providers,
        active_providers,
        total_api_keys,
        active_api_keys,
        today_requests,
        today_tokens,
        today_cost,
        INTERNAL_GATEWAY_PATH_PREFIXES,
        recent_errors,
        usage_counter,
    ))
}
