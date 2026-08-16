use std::time::{SystemTime, UNIX_EPOCH};

use aether_data_contracts::repository::usage::{StoredRequestUsageAudit, UpsertUsageRecord};
use aether_data_contracts::DataLayerError;
use serde_json::{Map, Value};
use sqlx::{QueryBuilder, Row, Sqlite, Transaction};

use crate::error::SqlResultExt;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct UsageRoutingSnapshot {
    candidate_id: Option<String>,
    candidate_index: Option<u64>,
    key_name: Option<String>,
    planner_kind: Option<String>,
    route_family: Option<String>,
    route_kind: Option<String>,
    execution_path: Option<String>,
    local_execution_runtime_miss_reason: Option<String>,
    selected_provider_id: Option<String>,
    selected_endpoint_id: Option<String>,
    selected_provider_api_key_id: Option<String>,
}

impl UsageRoutingSnapshot {
    fn has_metadata_fields(&self) -> bool {
        self.candidate_id.is_some()
            || self.candidate_index.is_some()
            || self.key_name.is_some()
            || self.planner_kind.is_some()
            || self.route_family.is_some()
            || self.route_kind.is_some()
            || self.execution_path.is_some()
            || self.local_execution_runtime_miss_reason.is_some()
    }

    fn any_present(&self) -> bool {
        self.has_metadata_fields()
            || self.selected_provider_id.is_some()
            || self.selected_endpoint_id.is_some()
            || self.selected_provider_api_key_id.is_some()
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct UsageSettlementPricingSnapshot {
    billing_status: Option<String>,
    billing_snapshot_schema_version: Option<String>,
    billing_snapshot_status: Option<String>,
    settlement_snapshot_schema_version: Option<String>,
    settlement_snapshot: Option<Value>,
    billing_dimensions: Option<Value>,
    billing_input_tokens: Option<i64>,
    billing_effective_input_tokens: Option<i64>,
    billing_output_tokens: Option<i64>,
    billing_cache_creation_tokens: Option<i64>,
    billing_cache_creation_5m_tokens: Option<i64>,
    billing_cache_creation_1h_tokens: Option<i64>,
    billing_cache_read_tokens: Option<i64>,
    billing_total_input_context: Option<i64>,
    billing_cache_creation_cost_usd: Option<f64>,
    billing_cache_read_cost_usd: Option<f64>,
    billing_total_cost_usd: Option<f64>,
    billing_actual_total_cost_usd: Option<f64>,
    billing_pricing_source: Option<String>,
    billing_rule_id: Option<String>,
    billing_rule_version: Option<String>,
    rate_multiplier: Option<f64>,
    input_price_per_1m: Option<f64>,
    output_price_per_1m: Option<f64>,
    cache_creation_price_per_1m: Option<f64>,
    cache_read_price_per_1m: Option<f64>,
    price_per_request: Option<f64>,
}

impl UsageSettlementPricingSnapshot {
    fn any_present(&self) -> bool {
        self.billing_status.is_some()
            || self.billing_snapshot_schema_version.is_some()
            || self.billing_snapshot_status.is_some()
            || self.settlement_snapshot_schema_version.is_some()
            || self.settlement_snapshot.is_some()
            || self.billing_dimensions.is_some()
            || self.billing_input_tokens.is_some()
            || self.billing_effective_input_tokens.is_some()
            || self.billing_output_tokens.is_some()
            || self.billing_cache_creation_tokens.is_some()
            || self.billing_cache_creation_5m_tokens.is_some()
            || self.billing_cache_creation_1h_tokens.is_some()
            || self.billing_cache_read_tokens.is_some()
            || self.billing_total_input_context.is_some()
            || self.billing_cache_creation_cost_usd.is_some()
            || self.billing_cache_read_cost_usd.is_some()
            || self.billing_total_cost_usd.is_some()
            || self.billing_actual_total_cost_usd.is_some()
            || self.billing_pricing_source.is_some()
            || self.billing_rule_id.is_some()
            || self.billing_rule_version.is_some()
            || self.rate_multiplier.is_some()
            || self.input_price_per_1m.is_some()
            || self.output_price_per_1m.is_some()
            || self.cache_creation_price_per_1m.is_some()
            || self.cache_read_price_per_1m.is_some()
            || self.price_per_request.is_some()
    }
}

pub(crate) fn from_usage(
    usage: &UpsertUsageRecord,
) -> Result<(UsageRoutingSnapshot, UsageSettlementPricingSnapshot), DataLayerError> {
    Ok((
        routing_snapshot_from_usage(usage),
        settlement_snapshot_from_usage(usage)?,
    ))
}

fn routing_snapshot_from_usage(usage: &UpsertUsageRecord) -> UsageRoutingSnapshot {
    let metadata = usage.request_metadata.as_ref().and_then(Value::as_object);
    let mut snapshot = UsageRoutingSnapshot {
        candidate_id: usage
            .candidate_id
            .clone()
            .or_else(|| metadata_string(metadata, "candidate_id")),
        candidate_index: usage
            .candidate_index
            .or_else(|| metadata_u64(metadata, "candidate_index")),
        key_name: usage
            .key_name
            .clone()
            .or_else(|| metadata_string(metadata, "key_name")),
        planner_kind: usage
            .planner_kind
            .clone()
            .or_else(|| metadata_string(metadata, "planner_kind")),
        route_family: usage
            .route_family
            .clone()
            .or_else(|| metadata_string(metadata, "route_family")),
        route_kind: usage
            .route_kind
            .clone()
            .or_else(|| metadata_string(metadata, "route_kind")),
        execution_path: usage
            .execution_path
            .clone()
            .or_else(|| metadata_string(metadata, "execution_path")),
        local_execution_runtime_miss_reason: usage
            .local_execution_runtime_miss_reason
            .clone()
            .or_else(|| metadata_string(metadata, "local_execution_runtime_miss_reason")),
        selected_provider_id: None,
        selected_endpoint_id: None,
        selected_provider_api_key_id: None,
    };
    if snapshot.has_metadata_fields() {
        snapshot.selected_provider_id = usage.provider_id.clone();
        snapshot.selected_endpoint_id = usage.provider_endpoint_id.clone();
        snapshot.selected_provider_api_key_id = usage.provider_api_key_id.clone();
    }
    snapshot
}

fn settlement_snapshot_from_usage(
    usage: &UpsertUsageRecord,
) -> Result<UsageSettlementPricingSnapshot, DataLayerError> {
    let metadata = usage.request_metadata.as_ref().and_then(Value::as_object);
    let billing_dimensions = metadata_or_snapshot_dimensions(metadata);
    let has_billing_dimensions = billing_dimensions.is_some();
    let usage_input_tokens = optional_i64(usage.input_tokens, "input_tokens")?;
    let usage_output_tokens = optional_i64(usage.output_tokens, "output_tokens")?;
    let usage_cache_creation_uncategorized_tokens = optional_i64(
        usage.cache_creation_input_tokens,
        "cache_creation_input_tokens",
    )?;
    let usage_cache_creation_5m_tokens = optional_i64(
        usage.cache_creation_ephemeral_5m_input_tokens,
        "cache_creation_ephemeral_5m_input_tokens",
    )?;
    let usage_cache_creation_1h_tokens = optional_i64(
        usage.cache_creation_ephemeral_1h_input_tokens,
        "cache_creation_ephemeral_1h_input_tokens",
    )?;
    let usage_cache_read_tokens =
        optional_i64(usage.cache_read_input_tokens, "cache_read_input_tokens")?;
    let usage_cache_creation_tokens = cache_creation_tokens_from_parts(
        usage_cache_creation_uncategorized_tokens,
        usage_cache_creation_5m_tokens,
        usage_cache_creation_1h_tokens,
    );
    let billing_cache_creation_tokens = billing_dimension_i64(metadata, "cache_creation_tokens")
        .or_else(|| {
            cache_creation_tokens_from_parts(
                billing_dimension_i64(metadata, "cache_creation_uncategorized_tokens"),
                billing_dimension_i64(metadata, "cache_creation_ephemeral_5m_tokens"),
                billing_dimension_i64(metadata, "cache_creation_ephemeral_1h_tokens"),
            )
        })
        .or(usage_cache_creation_tokens);
    let billing_cache_creation_5m_tokens =
        billing_dimension_i64(metadata, "cache_creation_ephemeral_5m_tokens")
            .or(usage_cache_creation_5m_tokens);
    let billing_cache_creation_1h_tokens =
        billing_dimension_i64(metadata, "cache_creation_ephemeral_1h_tokens")
            .or(usage_cache_creation_1h_tokens);
    let billing_input_tokens =
        billing_dimension_i64(metadata, "input_tokens").or(usage_input_tokens);
    let billing_output_tokens =
        billing_dimension_i64(metadata, "output_tokens").or(usage_output_tokens);
    let billing_cache_read_tokens =
        billing_dimension_i64(metadata, "cache_read_tokens").or(usage_cache_read_tokens);
    let api_family = normalized_api_family(usage);
    let billing_effective_input_tokens = billing_dimension_i64(metadata, "effective_input_tokens")
        .or_else(|| {
            has_billing_dimensions
                .then(|| billing_dimension_i64(metadata, "input_tokens"))
                .flatten()
        })
        .or_else(|| {
            effective_input_tokens(
                billing_input_tokens,
                billing_cache_creation_tokens,
                billing_cache_read_tokens,
                &api_family,
            )
        });
    let billing_total_input_context = billing_dimension_i64(metadata, "total_input_context")
        .or_else(|| {
            total_input_context(
                billing_input_tokens,
                billing_effective_input_tokens,
                billing_cache_creation_tokens,
                billing_cache_read_tokens,
                &api_family,
            )
        });

    Ok(UsageSettlementPricingSnapshot {
        billing_status: Some(usage.billing_status.clone()),
        billing_snapshot_schema_version: metadata_string(
            metadata,
            "billing_snapshot_schema_version",
        )
        .or_else(|| billing_snapshot_string(metadata, "schema_version")),
        billing_snapshot_status: metadata_string(metadata, "billing_snapshot_status")
            .or_else(|| billing_snapshot_string(metadata, "status")),
        settlement_snapshot_schema_version: settlement_snapshot_schema_version(metadata),
        settlement_snapshot: settlement_snapshot_value(metadata),
        billing_dimensions,
        billing_input_tokens,
        billing_effective_input_tokens,
        billing_output_tokens,
        billing_cache_creation_tokens,
        billing_cache_creation_5m_tokens,
        billing_cache_creation_1h_tokens,
        billing_cache_read_tokens,
        billing_total_input_context,
        billing_cache_creation_cost_usd: settlement_cache_creation_cost(metadata)
            .or(usage.cache_creation_cost_usd),
        billing_cache_read_cost_usd: settlement_cost_breakdown_number(metadata, "cache_read_cost")
            .or(usage.cache_read_cost_usd),
        billing_total_cost_usd: settlement_snapshot_number(metadata, "total_cost")
            .or_else(|| billing_snapshot_number(metadata, "total_cost"))
            .or(usage.total_cost_usd),
        billing_actual_total_cost_usd: settlement_snapshot_number(metadata, "actual_total_cost")
            .or(usage.actual_total_cost_usd),
        billing_pricing_source: settlement_nested_string(
            metadata,
            "pricing_snapshot",
            "pricing_source",
        ),
        billing_rule_id: settlement_nested_string(metadata, "billing_rule_snapshot", "rule_id")
            .or_else(|| billing_snapshot_string_field(metadata, "rule_id")),
        billing_rule_version: settlement_nested_string(
            metadata,
            "billing_rule_snapshot",
            "rule_version",
        ),
        rate_multiplier: metadata_number(metadata, "rate_multiplier"),
        input_price_per_1m: metadata_number(metadata, "input_price_per_1m")
            .or_else(|| billing_snapshot_resolved_number(metadata, "input_price_per_1m")),
        output_price_per_1m: metadata_number(metadata, "output_price_per_1m")
            .or_else(|| billing_snapshot_resolved_number(metadata, "output_price_per_1m"))
            .or(usage.output_price_per_1m),
        cache_creation_price_per_1m: metadata_number(metadata, "cache_creation_price_per_1m")
            .or_else(|| billing_snapshot_resolved_number(metadata, "cache_creation_price_per_1m")),
        cache_read_price_per_1m: metadata_number(metadata, "cache_read_price_per_1m")
            .or_else(|| billing_snapshot_resolved_number(metadata, "cache_read_price_per_1m")),
        price_per_request: metadata_number(metadata, "price_per_request")
            .or_else(|| billing_snapshot_resolved_number(metadata, "price_per_request")),
    })
}

pub(crate) async fn sync(
    tx: &mut Transaction<'_, Sqlite>,
    request_id: &str,
    routing: &UsageRoutingSnapshot,
    settlement: &UsageSettlementPricingSnapshot,
    replace_existing: bool,
) -> Result<(), DataLayerError> {
    sync_routing(tx, request_id, routing, replace_existing).await?;
    sync_settlement(tx, request_id, settlement, replace_existing).await
}

async fn sync_routing(
    tx: &mut Transaction<'_, Sqlite>,
    request_id: &str,
    snapshot: &UsageRoutingSnapshot,
    replace_existing: bool,
) -> Result<(), DataLayerError> {
    if !snapshot.any_present() && !replace_existing {
        return Ok(());
    }
    let now = unix_now()?;
    let mut query = QueryBuilder::<Sqlite>::new(
        "INSERT INTO usage_routing_snapshots (request_id, candidate_id, candidate_index, \
         key_name, planner_kind, route_family, route_kind, execution_path, \
         local_execution_runtime_miss_reason, selected_provider_id, selected_endpoint_id, \
         selected_provider_api_key_id, created_at, updated_at) VALUES (",
    );
    {
        let mut values = query.separated(", ");
        values
            .push_bind(request_id)
            .push_bind(snapshot.candidate_id.as_deref())
            .push_bind(optional_i64(snapshot.candidate_index, "candidate_index")?)
            .push_bind(snapshot.key_name.as_deref())
            .push_bind(snapshot.planner_kind.as_deref())
            .push_bind(snapshot.route_family.as_deref())
            .push_bind(snapshot.route_kind.as_deref())
            .push_bind(snapshot.execution_path.as_deref())
            .push_bind(snapshot.local_execution_runtime_miss_reason.as_deref())
            .push_bind(snapshot.selected_provider_id.as_deref())
            .push_bind(snapshot.selected_endpoint_id.as_deref())
            .push_bind(snapshot.selected_provider_api_key_id.as_deref())
            .push_bind(now)
            .push_bind(now);
    }
    query.push(") ON CONFLICT (request_id) DO UPDATE SET ");
    push_sqlite_updates(
        &mut query,
        &[
            "candidate_id",
            "candidate_index",
            "key_name",
            "planner_kind",
            "route_family",
            "route_kind",
            "execution_path",
            "local_execution_runtime_miss_reason",
            "selected_provider_id",
            "selected_endpoint_id",
            "selected_provider_api_key_id",
        ],
        "usage_routing_snapshots",
        replace_existing,
    );
    query.push(", updated_at = excluded.updated_at");
    query.build().execute(&mut **tx).await.map_sql_err()?;
    Ok(())
}

async fn sync_settlement(
    tx: &mut Transaction<'_, Sqlite>,
    request_id: &str,
    snapshot: &UsageSettlementPricingSnapshot,
    replace_existing: bool,
) -> Result<(), DataLayerError> {
    if !snapshot.any_present() && !replace_existing {
        return Ok(());
    }
    let now = unix_now()?;
    let settlement_json = json_text(snapshot.settlement_snapshot.as_ref())?;
    let dimensions_json = json_text(snapshot.billing_dimensions.as_ref())?;
    let mut query = QueryBuilder::<Sqlite>::new(
        "INSERT INTO usage_settlement_snapshots (request_id, billing_status, \
         billing_snapshot_schema_version, billing_snapshot_status, \
         settlement_snapshot_schema_version, settlement_snapshot, billing_dimensions, \
         billing_input_tokens, billing_effective_input_tokens, billing_output_tokens, \
         billing_cache_creation_tokens, billing_cache_creation_5m_tokens, \
         billing_cache_creation_1h_tokens, billing_cache_read_tokens, \
         billing_total_input_context, billing_cache_creation_cost_usd, \
         billing_cache_read_cost_usd, billing_total_cost_usd, \
         billing_actual_total_cost_usd, billing_pricing_source, billing_rule_id, \
         billing_rule_version, rate_multiplier, input_price_per_1m, \
         output_price_per_1m, cache_creation_price_per_1m, cache_read_price_per_1m, \
         price_per_request, created_at, updated_at) VALUES (",
    );
    {
        let mut values = query.separated(", ");
        values
            .push_bind(request_id)
            .push_bind(snapshot.billing_status.as_deref().unwrap_or("pending"))
            .push_bind(snapshot.billing_snapshot_schema_version.as_deref())
            .push_bind(snapshot.billing_snapshot_status.as_deref())
            .push_bind(snapshot.settlement_snapshot_schema_version.as_deref())
            .push_bind(settlement_json.as_deref())
            .push_bind(dimensions_json.as_deref())
            .push_bind(snapshot.billing_input_tokens)
            .push_bind(snapshot.billing_effective_input_tokens)
            .push_bind(snapshot.billing_output_tokens)
            .push_bind(snapshot.billing_cache_creation_tokens)
            .push_bind(snapshot.billing_cache_creation_5m_tokens)
            .push_bind(snapshot.billing_cache_creation_1h_tokens)
            .push_bind(snapshot.billing_cache_read_tokens)
            .push_bind(snapshot.billing_total_input_context)
            .push_bind(snapshot.billing_cache_creation_cost_usd)
            .push_bind(snapshot.billing_cache_read_cost_usd)
            .push_bind(snapshot.billing_total_cost_usd)
            .push_bind(snapshot.billing_actual_total_cost_usd)
            .push_bind(snapshot.billing_pricing_source.as_deref())
            .push_bind(snapshot.billing_rule_id.as_deref())
            .push_bind(snapshot.billing_rule_version.as_deref())
            .push_bind(snapshot.rate_multiplier)
            .push_bind(snapshot.input_price_per_1m)
            .push_bind(snapshot.output_price_per_1m)
            .push_bind(snapshot.cache_creation_price_per_1m)
            .push_bind(snapshot.cache_read_price_per_1m)
            .push_bind(snapshot.price_per_request)
            .push_bind(now)
            .push_bind(now);
    }
    query.push(") ON CONFLICT (request_id) DO UPDATE SET ");
    if replace_existing {
        query.push("billing_status = excluded.billing_status, ");
    }
    push_sqlite_updates(
        &mut query,
        &[
            "billing_snapshot_schema_version",
            "billing_snapshot_status",
            "settlement_snapshot_schema_version",
            "settlement_snapshot",
            "billing_dimensions",
            "billing_input_tokens",
            "billing_effective_input_tokens",
            "billing_output_tokens",
            "billing_cache_creation_tokens",
            "billing_cache_creation_5m_tokens",
            "billing_cache_creation_1h_tokens",
            "billing_cache_read_tokens",
            "billing_total_input_context",
            "billing_cache_creation_cost_usd",
            "billing_cache_read_cost_usd",
            "billing_total_cost_usd",
            "billing_actual_total_cost_usd",
            "billing_pricing_source",
            "billing_rule_id",
            "billing_rule_version",
            "rate_multiplier",
            "input_price_per_1m",
            "output_price_per_1m",
            "cache_creation_price_per_1m",
            "cache_read_price_per_1m",
            "price_per_request",
        ],
        "usage_settlement_snapshots",
        replace_existing,
    );
    query.push(", updated_at = excluded.updated_at");
    query.build().execute(&mut **tx).await.map_sql_err()?;
    Ok(())
}

fn push_sqlite_updates(
    query: &mut QueryBuilder<'_, Sqlite>,
    fields: &[&str],
    table: &str,
    replace_existing: bool,
) {
    for (index, field) in fields.iter().enumerate() {
        if index > 0 {
            query.push(", ");
        }
        query.push(*field).push(" = ");
        if replace_existing {
            query.push("excluded.").push(*field);
        } else {
            query
                .push("COALESCE(excluded.")
                .push(*field)
                .push(", ")
                .push(table)
                .push(".")
                .push(*field)
                .push(")");
        }
    }
}

pub(crate) fn hydrate_row(
    row: &sqlx::sqlite::SqliteRow,
    audit: &mut StoredRequestUsageAudit,
) -> Result<(), DataLayerError> {
    audit.candidate_id = row.try_get("routing_candidate_id").map_sql_err()?;
    audit.candidate_index = row
        .try_get::<Option<i64>, _>("routing_candidate_index")
        .map_sql_err()?
        .map(|value| {
            u64::try_from(value).map_err(|_| {
                DataLayerError::UnexpectedValue(format!(
                    "usage routing candidate_index is negative: {value}"
                ))
            })
        })
        .transpose()?;
    audit.key_name = row.try_get("routing_key_name").map_sql_err()?;
    audit.planner_kind = row.try_get("routing_planner_kind").map_sql_err()?;
    audit.route_family = row.try_get("routing_route_family").map_sql_err()?;
    audit.route_kind = row.try_get("routing_route_kind").map_sql_err()?;
    audit.execution_path = row.try_get("routing_execution_path").map_sql_err()?;
    audit.local_execution_runtime_miss_reason = row
        .try_get("routing_local_execution_runtime_miss_reason")
        .map_sql_err()?;

    let snapshot = settlement_snapshot_from_row(row)?;
    if let Some(effective) = nonnegative_u64(snapshot.billing_effective_input_tokens) {
        audit.total_tokens = effective
            .saturating_add(audit.output_tokens)
            .saturating_add(audit.cache_creation_input_tokens)
            .saturating_add(audit.cache_read_input_tokens);
    } else if let Some(context) = nonnegative_u64(snapshot.billing_total_input_context) {
        audit.total_tokens = context.saturating_add(audit.output_tokens);
    }
    audit.request_metadata = attach_settlement_metadata(audit.request_metadata.take(), &snapshot);
    Ok(())
}

fn settlement_snapshot_from_row(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<UsageSettlementPricingSnapshot, DataLayerError> {
    Ok(UsageSettlementPricingSnapshot {
        billing_status: None,
        billing_snapshot_schema_version: row
            .try_get("settlement_billing_snapshot_schema_version")
            .map_sql_err()?,
        billing_snapshot_status: row
            .try_get("settlement_billing_snapshot_status")
            .map_sql_err()?,
        settlement_snapshot_schema_version: row
            .try_get("settlement_snapshot_schema_version")
            .map_sql_err()?,
        settlement_snapshot: json_value_from_row(row, "settlement_snapshot")?,
        billing_dimensions: json_value_from_row(row, "settlement_billing_dimensions")?,
        billing_input_tokens: row
            .try_get("settlement_billing_input_tokens")
            .map_sql_err()?,
        billing_effective_input_tokens: row
            .try_get("settlement_billing_effective_input_tokens")
            .map_sql_err()?,
        billing_output_tokens: row
            .try_get("settlement_billing_output_tokens")
            .map_sql_err()?,
        billing_cache_creation_tokens: row
            .try_get("settlement_billing_cache_creation_tokens")
            .map_sql_err()?,
        billing_cache_creation_5m_tokens: row
            .try_get("settlement_billing_cache_creation_5m_tokens")
            .map_sql_err()?,
        billing_cache_creation_1h_tokens: row
            .try_get("settlement_billing_cache_creation_1h_tokens")
            .map_sql_err()?,
        billing_cache_read_tokens: row
            .try_get("settlement_billing_cache_read_tokens")
            .map_sql_err()?,
        billing_total_input_context: row
            .try_get("settlement_billing_total_input_context")
            .map_sql_err()?,
        billing_cache_creation_cost_usd: row
            .try_get("settlement_billing_cache_creation_cost_usd")
            .map_sql_err()?,
        billing_cache_read_cost_usd: row
            .try_get("settlement_billing_cache_read_cost_usd")
            .map_sql_err()?,
        billing_total_cost_usd: row
            .try_get("settlement_billing_total_cost_usd")
            .map_sql_err()?,
        billing_actual_total_cost_usd: row
            .try_get("settlement_billing_actual_total_cost_usd")
            .map_sql_err()?,
        billing_pricing_source: row
            .try_get("settlement_billing_pricing_source")
            .map_sql_err()?,
        billing_rule_id: row.try_get("settlement_billing_rule_id").map_sql_err()?,
        billing_rule_version: row
            .try_get("settlement_billing_rule_version")
            .map_sql_err()?,
        rate_multiplier: row.try_get("settlement_rate_multiplier").map_sql_err()?,
        input_price_per_1m: row.try_get("settlement_input_price_per_1m").map_sql_err()?,
        output_price_per_1m: row
            .try_get("settlement_output_price_per_1m")
            .map_sql_err()?,
        cache_creation_price_per_1m: row
            .try_get("settlement_cache_creation_price_per_1m")
            .map_sql_err()?,
        cache_read_price_per_1m: row
            .try_get("settlement_cache_read_price_per_1m")
            .map_sql_err()?,
        price_per_request: row.try_get("settlement_price_per_request").map_sql_err()?,
    })
}

fn attach_settlement_metadata(
    metadata: Option<Value>,
    snapshot: &UsageSettlementPricingSnapshot,
) -> Option<Value> {
    if !snapshot.any_present() {
        return metadata;
    }
    let mut metadata = match metadata {
        Some(Value::Object(object)) => object,
        Some(value) => return Some(value),
        None => Map::new(),
    };
    insert_string(
        &mut metadata,
        "billing_snapshot_schema_version",
        snapshot.billing_snapshot_schema_version.as_deref(),
    );
    insert_string(
        &mut metadata,
        "billing_snapshot_status",
        snapshot.billing_snapshot_status.as_deref(),
    );
    insert_string(
        &mut metadata,
        "settlement_snapshot_schema_version",
        snapshot.settlement_snapshot_schema_version.as_deref(),
    );
    insert_value(
        &mut metadata,
        "settlement_snapshot",
        snapshot.settlement_snapshot.as_ref(),
    );
    insert_value(
        &mut metadata,
        "billing_dimensions",
        snapshot.billing_dimensions.as_ref(),
    );
    insert_number(&mut metadata, "rate_multiplier", snapshot.rate_multiplier);
    insert_number(
        &mut metadata,
        "input_price_per_1m",
        snapshot.input_price_per_1m,
    );
    insert_number(
        &mut metadata,
        "output_price_per_1m",
        snapshot.output_price_per_1m,
    );
    insert_number(
        &mut metadata,
        "cache_creation_price_per_1m",
        snapshot.cache_creation_price_per_1m,
    );
    insert_number(
        &mut metadata,
        "cache_read_price_per_1m",
        snapshot.cache_read_price_per_1m,
    );
    insert_number(
        &mut metadata,
        "price_per_request",
        snapshot.price_per_request,
    );
    (!metadata.is_empty()).then_some(Value::Object(metadata))
}

fn insert_string(metadata: &mut Map<String, Value>, key: &str, value: Option<&str>) {
    if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
        metadata.insert(key.to_string(), Value::String(value.to_string()));
    }
}

fn insert_number(metadata: &mut Map<String, Value>, key: &str, value: Option<f64>) {
    if let Some(number) = value
        .filter(|value| value.is_finite())
        .and_then(serde_json::Number::from_f64)
    {
        metadata.insert(key.to_string(), Value::Number(number));
    }
}

fn insert_value(metadata: &mut Map<String, Value>, key: &str, value: Option<&Value>) {
    if let Some(value) = value {
        metadata.insert(key.to_string(), value.clone());
    }
}

fn metadata_string(metadata: Option<&Map<String, Value>>, key: &str) -> Option<String> {
    metadata
        .and_then(|object| object.get(key))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn metadata_number(metadata: Option<&Map<String, Value>>, key: &str) -> Option<f64> {
    metadata
        .and_then(|object| object.get(key))
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
}

fn metadata_u64(metadata: Option<&Map<String, Value>>, key: &str) -> Option<u64> {
    metadata.and_then(|object| {
        object.get(key).and_then(|value| {
            value
                .as_u64()
                .or_else(|| value.as_i64().and_then(|number| u64::try_from(number).ok()))
        })
    })
}

fn billing_snapshot_object(metadata: Option<&Map<String, Value>>) -> Option<&Map<String, Value>> {
    metadata
        .and_then(|object| object.get("billing_snapshot"))
        .and_then(Value::as_object)
}

fn billing_snapshot_string(metadata: Option<&Map<String, Value>>, key: &str) -> Option<String> {
    billing_snapshot_object(metadata)
        .and_then(|snapshot| snapshot.get(key))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn billing_snapshot_resolved_number(
    metadata: Option<&Map<String, Value>>,
    key: &str,
) -> Option<f64> {
    billing_snapshot_object(metadata)
        .and_then(|snapshot| snapshot.get("resolved_variables"))
        .and_then(Value::as_object)
        .and_then(|variables| variables.get(key))
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
}

fn settlement_snapshot_object(
    metadata: Option<&Map<String, Value>>,
) -> Option<&Map<String, Value>> {
    metadata
        .and_then(|object| object.get("settlement_snapshot"))
        .and_then(Value::as_object)
}

fn settlement_snapshot_schema_version(metadata: Option<&Map<String, Value>>) -> Option<String> {
    metadata_string(metadata, "settlement_snapshot_schema_version").or_else(|| {
        settlement_snapshot_object(metadata)
            .and_then(|snapshot| snapshot.get("schema_version"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
    })
}

fn settlement_snapshot_value(metadata: Option<&Map<String, Value>>) -> Option<Value> {
    metadata
        .and_then(|object| object.get("settlement_snapshot"))
        .cloned()
}

fn settlement_child_value<'a>(
    metadata: Option<&'a Map<String, Value>>,
    child: &str,
) -> Option<&'a Value> {
    settlement_snapshot_object(metadata).and_then(|snapshot| snapshot.get(child))
}

fn settlement_child_object<'a>(
    metadata: Option<&'a Map<String, Value>>,
    child: &str,
) -> Option<&'a Map<String, Value>> {
    settlement_child_value(metadata, child).and_then(Value::as_object)
}

fn metadata_or_snapshot_dimensions(metadata: Option<&Map<String, Value>>) -> Option<Value> {
    metadata
        .and_then(|object| object.get("billing_dimensions"))
        .cloned()
        .or_else(|| settlement_child_value(metadata, "resolved_dimensions").cloned())
        .or_else(|| {
            billing_snapshot_object(metadata)
                .and_then(|snapshot| snapshot.get("resolved_dimensions"))
                .cloned()
        })
}

fn billing_dimension_i64(metadata: Option<&Map<String, Value>>, key: &str) -> Option<i64> {
    metadata_or_snapshot_dimensions(metadata)
        .and_then(|dimensions| dimensions.get(key).and_then(json_i64))
        .filter(|value| *value >= 0)
}

fn json_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|number| i64::try_from(number).ok()))
}

fn settlement_snapshot_number(metadata: Option<&Map<String, Value>>, key: &str) -> Option<f64> {
    settlement_snapshot_object(metadata)
        .and_then(|snapshot| snapshot.get(key))
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
}

fn billing_snapshot_number(metadata: Option<&Map<String, Value>>, key: &str) -> Option<f64> {
    billing_snapshot_object(metadata)
        .and_then(|snapshot| snapshot.get(key))
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
}

fn settlement_cost_breakdown_number(
    metadata: Option<&Map<String, Value>>,
    key: &str,
) -> Option<f64> {
    settlement_child_object(metadata, "cost_breakdown")
        .or_else(|| {
            billing_snapshot_object(metadata)
                .and_then(|snapshot| snapshot.get("cost_breakdown"))
                .and_then(Value::as_object)
        })
        .and_then(|breakdown| breakdown.get(key))
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
}

fn settlement_cache_creation_cost(metadata: Option<&Map<String, Value>>) -> Option<f64> {
    let mut found = false;
    let total = [
        "cache_creation_uncategorized_cost",
        "cache_creation_ephemeral_5m_cost",
        "cache_creation_ephemeral_1h_cost",
        "cache_creation_cost",
    ]
    .into_iter()
    .fold(0.0, |sum, key| {
        if let Some(value) = settlement_cost_breakdown_number(metadata, key) {
            found = true;
            sum + value
        } else {
            sum
        }
    });
    found.then_some(total)
}

fn settlement_nested_string(
    metadata: Option<&Map<String, Value>>,
    child: &str,
    key: &str,
) -> Option<String> {
    settlement_child_object(metadata, child)
        .and_then(|object| object.get(key))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn billing_snapshot_string_field(
    metadata: Option<&Map<String, Value>>,
    key: &str,
) -> Option<String> {
    billing_snapshot_object(metadata)
        .and_then(|snapshot| snapshot.get(key))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn optional_i64(value: Option<u64>, field: &str) -> Result<Option<i64>, DataLayerError> {
    value
        .map(|value| {
            i64::try_from(value).map_err(|_| {
                DataLayerError::UnexpectedValue(format!("usage {field} exceeds bigint: {value}"))
            })
        })
        .transpose()
}

fn cache_creation_tokens_from_parts(
    uncategorized: Option<i64>,
    ephemeral_5m: Option<i64>,
    ephemeral_1h: Option<i64>,
) -> Option<i64> {
    let categorized = ephemeral_5m
        .unwrap_or_default()
        .saturating_add(ephemeral_1h.unwrap_or_default());
    match uncategorized {
        Some(0) if categorized > 0 => Some(categorized),
        Some(value) => Some(value),
        None if categorized > 0 => Some(categorized),
        None => None,
    }
}

fn normalized_api_family(usage: &UpsertUsageRecord) -> String {
    usage
        .endpoint_api_format
        .as_deref()
        .or(usage.api_format.as_deref())
        .unwrap_or_default()
        .split(':')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
}

fn effective_input_tokens(
    input_tokens: Option<i64>,
    cache_creation_tokens: Option<i64>,
    cache_read_tokens: Option<i64>,
    api_family: &str,
) -> Option<i64> {
    let input_tokens = input_tokens?;
    let cache_creation_tokens = cache_creation_tokens.unwrap_or_default();
    let cache_read_tokens = cache_read_tokens.unwrap_or_default();
    if input_tokens > 0 {
        if api_family == "openai" && (cache_creation_tokens > 0 || cache_read_tokens > 0) {
            return Some(
                input_tokens
                    .saturating_sub(cache_creation_tokens)
                    .saturating_sub(cache_read_tokens),
            );
        }
        if matches!(api_family, "gemini" | "google") && cache_read_tokens > 0 {
            return Some(input_tokens.saturating_sub(cache_read_tokens));
        }
    }
    Some(input_tokens)
}

fn total_input_context(
    input_tokens: Option<i64>,
    effective_input_tokens: Option<i64>,
    cache_creation_tokens: Option<i64>,
    cache_read_tokens: Option<i64>,
    api_family: &str,
) -> Option<i64> {
    if input_tokens.is_none()
        && effective_input_tokens.is_none()
        && cache_creation_tokens.is_none()
        && cache_read_tokens.is_none()
    {
        return None;
    }
    let input_tokens = input_tokens.unwrap_or_default();
    let effective_input_tokens = effective_input_tokens.unwrap_or(input_tokens);
    let cache_creation_tokens = cache_creation_tokens.unwrap_or_default();
    let cache_read_tokens = cache_read_tokens.unwrap_or_default();
    match api_family {
        "claude" | "anthropic" => Some(
            input_tokens
                .saturating_add(cache_creation_tokens)
                .saturating_add(cache_read_tokens),
        ),
        "openai" => Some(
            effective_input_tokens
                .saturating_add(cache_creation_tokens)
                .saturating_add(cache_read_tokens),
        ),
        "gemini" | "google" => Some(effective_input_tokens.saturating_add(cache_read_tokens)),
        _ => Some(
            input_tokens
                .saturating_add(cache_creation_tokens)
                .saturating_add(cache_read_tokens),
        ),
    }
}

fn json_text(value: Option<&Value>) -> Result<Option<String>, DataLayerError> {
    value
        .map(|value| {
            serde_json::to_string(value).map_err(|error| {
                DataLayerError::UnexpectedValue(format!(
                    "failed to serialize usage settlement snapshot: {error}"
                ))
            })
        })
        .transpose()
}

fn json_value_from_row(
    row: &sqlx::sqlite::SqliteRow,
    column: &str,
) -> Result<Option<Value>, DataLayerError> {
    row.try_get::<Option<String>, _>(column)
        .map_sql_err()?
        .map(|value| {
            serde_json::from_str(&value).map_err(|error| {
                DataLayerError::UnexpectedValue(format!(
                    "invalid usage settlement JSON in {column}: {error}"
                ))
            })
        })
        .transpose()
}

fn nonnegative_u64(value: Option<i64>) -> Option<u64> {
    value.and_then(|value| u64::try_from(value).ok())
}

fn unix_now() -> Result<i64, DataLayerError> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| DataLayerError::UnexpectedValue(error.to_string()))?
        .as_secs();
    i64::try_from(seconds)
        .map_err(|_| DataLayerError::UnexpectedValue("unix timestamp overflow".to_string()))
}
