use super::super::super::stats::resolve_admin_usage_time_range;
use super::super::analytics::admin_usage_aggregation_by_user_json;
use crate::handlers::admin::request::{AdminAppState, AdminRequestContext};
use crate::handlers::admin::shared::query_param_value;
use crate::GatewayError;
use aether_admin::observability::stats::round_to;
use aether_admin::observability::usage::{
    admin_usage_bad_request_response, admin_usage_data_unavailable_response,
    admin_usage_parse_aggregation_limit, admin_usage_token_cache_hit_rate,
    ADMIN_USAGE_DATA_UNAVAILABLE_DETAIL,
};
use aether_data_contracts::repository::usage::{
    StoredUsageAuditAggregation, StoredUsageBreakdownSummaryRow, UsageAuditAggregationGroupBy,
    UsageAuditAggregationQuery, UsageBreakdownGroupBy, UsageBreakdownSummaryQuery,
};
use axum::{
    body::Body,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

fn admin_usage_aggregation_by_model_json(
    rows: &[StoredUsageAuditAggregation],
) -> serde_json::Value {
    json!(rows
        .iter()
        .map(|row| {
            json!({
                "model": row.group_key,
                "request_count": row.request_count,
                "total_tokens": row.total_tokens,
                "effective_input_tokens": row.effective_input_tokens,
                "total_input_context": row.total_input_context,
                "output_tokens": row.output_tokens,
                "total_cost": round_to(row.total_cost_usd, 6),
                "actual_cost": round_to(row.actual_total_cost_usd, 6),
                "cache_creation_tokens": row.cache_creation_tokens,
                "cache_creation_ephemeral_5m_tokens": row.cache_creation_ephemeral_5m_tokens,
                "cache_creation_ephemeral_1h_tokens": row.cache_creation_ephemeral_1h_tokens,
                "cache_read_tokens": row.cache_read_tokens,
                "cache_hit_rate": admin_usage_token_cache_hit_rate(
                    row.total_input_context,
                    row.cache_read_tokens,
                ),
            })
        })
        .collect::<Vec<_>>())
}

fn admin_usage_aggregation_by_provider_json(
    rows: &[StoredUsageAuditAggregation],
) -> serde_json::Value {
    json!(rows
        .iter()
        .map(|row| {
            let identity_source = match row.secondary_name.as_deref() {
                Some("legacy_name") => "legacy_name",
                _ => "provider_id",
            };
            let provider_id = if identity_source == "provider_id" {
                json!(row.group_key)
            } else {
                serde_json::Value::Null
            };
            let success_count = row.success_count.unwrap_or_default();
            let error_count = row.request_count.saturating_sub(success_count);
            let success_rate = if row.request_count == 0 {
                0.0
            } else {
                round_to(success_count as f64 / row.request_count as f64 * 100.0, 2)
            };
            let provider_name = row
                .display_name
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or(row.group_key.as_str());
            json!({
                "provider_id": provider_id,
                "provider_key": row.group_key,
                "provider_identity_source": identity_source,
                "provider": provider_name,
                "request_count": row.request_count,
                "total_tokens": row.total_tokens,
                "effective_input_tokens": row.effective_input_tokens,
                "total_input_context": row.total_input_context,
                "output_tokens": row.output_tokens,
                "total_cost": round_to(row.total_cost_usd, 6),
                "actual_cost": round_to(row.actual_total_cost_usd, 6),
                "avg_response_time_ms": round_to(row.avg_response_time_ms.unwrap_or(0.0), 2),
                "success_rate": success_rate,
                "error_count": error_count,
                "cache_creation_tokens": row.cache_creation_tokens,
                "cache_creation_ephemeral_5m_tokens": row.cache_creation_ephemeral_5m_tokens,
                "cache_creation_ephemeral_1h_tokens": row.cache_creation_ephemeral_1h_tokens,
                "cache_read_tokens": row.cache_read_tokens,
                "cache_hit_rate": admin_usage_token_cache_hit_rate(
                    row.total_input_context,
                    row.cache_read_tokens,
                ),
            })
        })
        .collect::<Vec<_>>())
}

fn admin_usage_aggregation_by_api_format_json(
    rows: &[StoredUsageAuditAggregation],
) -> serde_json::Value {
    json!(rows
        .iter()
        .map(|row| {
            json!({
                "api_format": row.group_key,
                "request_count": row.request_count,
                "total_tokens": row.total_tokens,
                "effective_input_tokens": row.effective_input_tokens,
                "total_input_context": row.total_input_context,
                "output_tokens": row.output_tokens,
                "total_cost": round_to(row.total_cost_usd, 6),
                "actual_cost": round_to(row.actual_total_cost_usd, 6),
                "avg_response_time_ms": round_to(row.avg_response_time_ms.unwrap_or(0.0), 2),
                "cache_creation_tokens": row.cache_creation_tokens,
                "cache_creation_ephemeral_5m_tokens": row.cache_creation_ephemeral_5m_tokens,
                "cache_creation_ephemeral_1h_tokens": row.cache_creation_ephemeral_1h_tokens,
                "cache_read_tokens": row.cache_read_tokens,
                "cache_hit_rate": admin_usage_token_cache_hit_rate(
                    row.total_input_context,
                    row.cache_read_tokens,
                ),
            })
        })
        .collect::<Vec<_>>())
}

fn usage_breakdown_aggregation_rows(
    rows: Vec<StoredUsageBreakdownSummaryRow>,
    group_by: UsageBreakdownGroupBy,
    limit: usize,
) -> Vec<StoredUsageAuditAggregation> {
    rows.into_iter()
        .filter(|row| {
            !matches!(
                row.group_key.trim().to_ascii_lowercase().as_str(),
                "" | "unknown" | "unknow" | "pending"
            )
        })
        .take(limit)
        .map(|row| {
            let avg_response_time_ms = (row.overall_response_time_samples > 0).then(|| {
                row.overall_response_time_sum_ms / row.overall_response_time_samples as f64
            });
            let provider_name =
                matches!(group_by, UsageBreakdownGroupBy::Provider).then(|| row.group_key.clone());
            StoredUsageAuditAggregation {
                group_key: row.group_key,
                display_name: provider_name,
                secondary_name: matches!(group_by, UsageBreakdownGroupBy::Provider)
                    .then(|| "legacy_name".to_string()),
                request_count: row.request_count,
                total_tokens: row.total_tokens,
                output_tokens: row.output_tokens,
                effective_input_tokens: row.effective_input_tokens,
                total_input_context: row.total_input_context,
                cache_creation_tokens: row.cache_creation_tokens,
                cache_creation_ephemeral_5m_tokens: row.cache_creation_ephemeral_5m_tokens,
                cache_creation_ephemeral_1h_tokens: row.cache_creation_ephemeral_1h_tokens,
                cache_read_tokens: row.cache_read_tokens,
                total_cost_usd: row.total_cost_usd,
                actual_total_cost_usd: row.actual_total_cost_usd,
                avg_response_time_ms,
                success_count: Some(row.success_count),
            }
        })
        .collect()
}

pub(super) async fn build_admin_usage_aggregation_stats_response(
    state: &AdminAppState<'_>,
    request_context: &AdminRequestContext<'_>,
) -> Result<Response<Body>, GatewayError> {
    if !state.has_usage_data_reader() {
        return Ok(admin_usage_data_unavailable_response(
            ADMIN_USAGE_DATA_UNAVAILABLE_DETAIL,
        ));
    }

    let query = request_context.request_query_string.as_deref();
    let group_by = query_param_value(query, "group_by")
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    if !matches!(
        group_by.as_str(),
        "model" | "user" | "provider" | "api_format"
    ) {
        return Ok(admin_usage_bad_request_response(
            "Invalid group_by value: must be one of model, user, provider, api_format",
        ));
    }
    let limit = match admin_usage_parse_aggregation_limit(query) {
        Ok(value) => value,
        Err(detail) => return Ok(admin_usage_bad_request_response(detail)),
    };
    let time_range = match resolve_admin_usage_time_range(query) {
        Ok(value) => value,
        Err(detail) => return Ok(admin_usage_bad_request_response(detail)),
    };
    let Some((created_from_unix_secs, created_until_unix_secs)) = time_range.to_unix_bounds()
    else {
        return Ok(Json(json!([])).into_response());
    };
    let user_id = query_param_value(query, "user_id")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let group_by_query = match group_by.as_str() {
        "model" => UsageAuditAggregationGroupBy::Model,
        "user" => UsageAuditAggregationGroupBy::User,
        "provider" => UsageAuditAggregationGroupBy::Provider,
        "api_format" => UsageAuditAggregationGroupBy::ApiFormat,
        _ => unreachable!(),
    };
    let exclude_reserved_provider_labels = matches!(
        group_by_query,
        UsageAuditAggregationGroupBy::Model
            | UsageAuditAggregationGroupBy::Provider
            | UsageAuditAggregationGroupBy::ApiFormat
    );
    let usage = if let Some(user_id) = user_id {
        let breakdown_group_by = match group_by.as_str() {
            "model" => Some(UsageBreakdownGroupBy::Model),
            "provider" => Some(UsageBreakdownGroupBy::Provider),
            "api_format" => Some(UsageBreakdownGroupBy::ApiFormat),
            _ => None,
        };
        if let Some(breakdown_group_by) = breakdown_group_by {
            let rows = state
                .summarize_usage_breakdown(&UsageBreakdownSummaryQuery {
                    created_from_unix_secs,
                    created_until_unix_secs,
                    user_id: Some(user_id),
                    provider_name: None,
                    model: None,
                    api_format: None,
                    exclude_status_codes: Vec::new(),
                    group_by: breakdown_group_by,
                })
                .await?;
            usage_breakdown_aggregation_rows(rows, breakdown_group_by, limit)
        } else {
            state
                .aggregate_usage_audits(&UsageAuditAggregationQuery {
                    created_from_unix_secs,
                    created_until_unix_secs,
                    group_by: group_by_query,
                    limit,
                    exclude_reserved_provider_labels,
                })
                .await?
        }
    } else {
        state
            .aggregate_usage_audits(&UsageAuditAggregationQuery {
                created_from_unix_secs,
                created_until_unix_secs,
                group_by: group_by_query,
                limit,
                exclude_reserved_provider_labels,
            })
            .await?
    };

    let response = match group_by.as_str() {
        "model" => admin_usage_aggregation_by_model_json(&usage),
        "user" => admin_usage_aggregation_by_user_json(state, &usage).await?,
        "provider" => admin_usage_aggregation_by_provider_json(&usage),
        "api_format" => admin_usage_aggregation_by_api_format_json(&usage),
        _ => unreachable!(),
    };
    Ok(Json(response).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_breakdown(group_key: &str, request_count: u64) -> StoredUsageBreakdownSummaryRow {
        StoredUsageBreakdownSummaryRow {
            group_key: group_key.to_string(),
            request_count,
            input_tokens: 20,
            total_tokens: 50,
            output_tokens: 30,
            effective_input_tokens: 20,
            total_input_context: 25,
            cache_creation_tokens: 3,
            cache_creation_ephemeral_5m_tokens: 2,
            cache_creation_ephemeral_1h_tokens: 1,
            cache_read_tokens: 5,
            total_cost_usd: 1.25,
            actual_total_cost_usd: 0.75,
            success_count: request_count.saturating_sub(1),
            response_time_sum_ms: 80.0,
            response_time_samples: 1,
            overall_response_time_sum_ms: 120.0,
            overall_response_time_samples: 2,
        }
    }

    #[test]
    fn user_breakdown_rows_preserve_usage_and_filter_reserved_groups() {
        let rows = usage_breakdown_aggregation_rows(
            vec![
                sample_breakdown("OpenAI", 4),
                sample_breakdown("unknown", 3),
            ],
            UsageBreakdownGroupBy::Provider,
            10,
        );

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].group_key, "OpenAI");
        assert_eq!(rows[0].display_name.as_deref(), Some("OpenAI"));
        assert_eq!(rows[0].request_count, 4);
        assert_eq!(rows[0].total_tokens, 50);
        assert_eq!(rows[0].avg_response_time_ms, Some(60.0));
    }
}
