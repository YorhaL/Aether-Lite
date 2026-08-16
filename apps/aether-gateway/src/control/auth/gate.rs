use axum::body::Bytes;
use axum::http::Uri;

use super::super::GatewayControlDecision;
use super::credentials::{contains_string, extract_requested_model};
use super::GatewayControlAuthContext;
use crate::stage_metrics::observe_gateway_stage_ms;
use crate::{AppState, GatewayError};

const BALANCE_EPSILON_USD: f64 = 0.000_000_01;
const AUTH_PRICING_VALIDATION_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(5);
// Billing mutations clear this cache locally. The bounded stale window leaves
// room for cross-node propagation without synchronously reloading at every
// short TTL boundary.
const AUTH_CAPACITY_CACHE_STALE_TTL: std::time::Duration = std::time::Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum GatewayLocalAuthRejection {
    InvalidApiKey,
    LockedApiKey,
    WalletUnavailable,
    BalanceDenied { remaining: Option<f64> },
    ProviderNotAllowed { provider: String },
    ApiFormatNotAllowed { api_format: String },
    ModelNotAllowed { model: String },
    IpNotAllowed { remote_ip: String },
}

pub(crate) fn trusted_auth_local_rejection(
    decision: Option<&GatewayControlDecision>,
    _headers: &http::HeaderMap,
) -> Option<GatewayLocalAuthRejection> {
    let decision = decision?;
    if decision.route_class.as_deref() != Some("ai_public") {
        return None;
    }

    decision
        .local_auth_rejection
        .clone()
        .or_else(|| decision.auth_context.as_ref()?.local_rejection.clone())
}

pub(crate) fn should_buffer_request_for_local_auth(
    decision: Option<&GatewayControlDecision>,
    headers: &http::HeaderMap,
) -> bool {
    let Some(decision) = decision else {
        return false;
    };
    decision.route_class.as_deref() == Some("ai_public")
        && decision.route_kind.as_deref() != Some("files")
        && crate::headers::is_json_request(headers)
}

pub(crate) async fn request_model_local_rejection(
    state: &AppState,
    decision: Option<&GatewayControlDecision>,
    uri: &Uri,
    headers: &http::HeaderMap,
    body: &Bytes,
) -> Result<Option<GatewayLocalAuthRejection>, GatewayError> {
    let Some(decision) = decision else {
        return Ok(None);
    };
    if decision.route_class.as_deref() != Some("ai_public") {
        return Ok(None);
    }
    let Some(auth_context) = decision.auth_context.as_ref() else {
        return Ok(None);
    };
    let requested_model = extract_requested_model(decision, uri, headers, body);
    if let (Some(allowed_models), Some(requested_model)) = (
        auth_context.allowed_models.as_deref(),
        requested_model.as_deref(),
    ) {
        if !contains_string(allowed_models, requested_model)
            && !model_directive_base_model_is_allowed_for_request(
                decision,
                requested_model,
                allowed_models,
            )
            && !request_model_resolves_to_allowed_model(
                state,
                decision,
                requested_model,
                allowed_models,
            )
            .await?
        {
            return Ok(Some(GatewayLocalAuthRejection::ModelNotAllowed {
                model: requested_model.to_string(),
            }));
        }
    }

    Ok(None)
}

pub(crate) async fn execution_plan_balance_capacity_rejection(
    state: &AppState,
    decision: &GatewayControlDecision,
    plan: &aether_contracts::ExecutionPlan,
    report_context: Option<&serde_json::Value>,
) -> Result<Option<GatewayLocalAuthRejection>, GatewayError> {
    let started_at = std::time::Instant::now();
    let result =
        execution_plan_balance_capacity_rejection_inner(state, decision, plan, report_context)
            .await;
    observe_gateway_stage_ms(
        "auth_capacity_total",
        started_at.elapsed().as_millis() as u64,
    );
    result
}

async fn execution_plan_balance_capacity_rejection_inner(
    state: &AppState,
    decision: &GatewayControlDecision,
    plan: &aether_contracts::ExecutionPlan,
    report_context: Option<&serde_json::Value>,
) -> Result<Option<GatewayLocalAuthRejection>, GatewayError> {
    let Some(auth_context) = decision.auth_context.as_ref() else {
        return Ok(None);
    };
    if auth_context.local_rejection.is_some() {
        return Ok(None);
    }
    if auth_context.api_key_is_standalone {
        validate_execution_plan_pricing_configuration_for_plan(state, plan, report_context).await?;
        return Ok(None);
    }
    let Some(available_usd) = available_balance_capacity_usd(state, auth_context).await? else {
        validate_execution_plan_pricing_configuration_for_plan(state, plan, report_context).await?;
        return Ok(None);
    };
    match estimate_execution_plan_cost_upper_bound_usd(state, plan, report_context).await? {
        Some(estimated_cost_usd) if estimated_cost_usd <= available_usd + BALANCE_EPSILON_USD => {
            Ok(None)
        }
        Some(_) | None if available_usd <= BALANCE_EPSILON_USD => {
            Ok(Some(GatewayLocalAuthRejection::BalanceDenied {
                remaining: Some(0.0),
            }))
        }
        Some(_) => Ok(Some(GatewayLocalAuthRejection::BalanceDenied {
            remaining: Some(available_usd),
        })),
        None => Ok(None),
    }
}

async fn validate_execution_plan_pricing_configuration_for_plan(
    state: &AppState,
    plan: &aether_contracts::ExecutionPlan,
    report_context: Option<&serde_json::Value>,
) -> Result<(), GatewayError> {
    let model_id = report_context_string_field(report_context, "model_id");
    let global_model_name = report_context_string_field(report_context, "global_model_name");
    let requested_processing_tier =
        aether_data_contracts::repository::usage::extract_provider_service_tier_from_body(
            plan.body.json_body.as_ref(),
        );
    validate_execution_plan_pricing_for_unavailable_estimate(
        state,
        plan,
        model_id,
        global_model_name,
        requested_processing_tier.as_deref(),
    )
    .await
}

async fn available_balance_capacity_usd(
    state: &AppState,
    auth_context: &GatewayControlAuthContext,
) -> Result<Option<f64>, GatewayError> {
    let wallet_started_at = std::time::Instant::now();
    let wallet_result = state
        .read_wallet_snapshot_for_auth(
            &auth_context.user_id,
            &auth_context.api_key_id,
            auth_context.api_key_is_standalone,
        )
        .await;
    observe_gateway_stage_ms(
        "auth_capacity_wallet",
        wallet_started_at.elapsed().as_millis() as u64,
    );
    let wallet = wallet_result?;
    let wallet_available_usd = wallet.as_ref().and_then(wallet_finite_available_usd);
    let wallet_is_unlimited = wallet
        .as_ref()
        .is_some_and(|wallet| wallet.limit_mode.eq_ignore_ascii_case("unlimited"));
    Ok(if wallet_is_unlimited {
        None
    } else {
        wallet_available_usd
    })
}

fn wallet_finite_available_usd(
    wallet: &aether_data::repository::wallet::StoredWalletSnapshot,
) -> Option<f64> {
    if !wallet.status.eq_ignore_ascii_case("active")
        || wallet.limit_mode.eq_ignore_ascii_case("unlimited")
    {
        return None;
    }
    Some(wallet.balance.max(0.0))
}

async fn estimate_execution_plan_cost_upper_bound_usd(
    state: &AppState,
    plan: &aether_contracts::ExecutionPlan,
    report_context: Option<&serde_json::Value>,
) -> Result<Option<f64>, GatewayError> {
    let started_at = std::time::Instant::now();
    let result =
        estimate_execution_plan_cost_upper_bound_usd_inner(state, plan, report_context).await;
    observe_gateway_stage_ms(
        "auth_capacity_cost_estimate",
        started_at.elapsed().as_millis() as u64,
    );
    result
}

pub(crate) async fn execution_plan_cost_is_proven_zero(
    state: &AppState,
    plan: &aether_contracts::ExecutionPlan,
    report_context: Option<&serde_json::Value>,
) -> bool {
    matches!(
        estimate_execution_plan_cost_upper_bound_usd(state, plan, report_context).await,
        Ok(Some(cost)) if cost <= BALANCE_EPSILON_USD
    )
}

async fn estimate_execution_plan_cost_upper_bound_usd_inner(
    state: &AppState,
    plan: &aether_contracts::ExecutionPlan,
    report_context: Option<&serde_json::Value>,
) -> Result<Option<f64>, GatewayError> {
    let api_format = crate::ai_serving::normalize_api_format_alias(&plan.provider_api_format);
    let body_json = plan.body.json_body.as_ref();
    let requested_processing_tier =
        aether_data_contracts::repository::usage::extract_provider_service_tier_from_body(
            body_json,
        );
    let model_id = report_context_string_field(report_context, "model_id");
    let global_model_name = report_context_string_field(report_context, "global_model_name");
    let Some(task_type) = authorization_task_type(&api_format, report_context) else {
        validate_execution_plan_pricing_for_unavailable_estimate(
            state,
            plan,
            model_id,
            global_model_name,
            requested_processing_tier.as_deref(),
        )
        .await?;
        return Ok(None);
    };
    let Some(body_json) = body_json else {
        validate_execution_plan_pricing_for_unavailable_estimate(
            state,
            plan,
            model_id,
            global_model_name,
            requested_processing_tier.as_deref(),
        )
        .await?;
        return Ok(None);
    };
    if !openai_request_input_is_self_contained(&api_format, body_json) {
        validate_execution_plan_pricing_for_unavailable_estimate(
            state,
            plan,
            model_id,
            global_model_name,
            requested_processing_tier.as_deref(),
        )
        .await?;
        return Ok(None);
    }
    let input_tokens = json_token_count_upper_bound(body_json);
    let Ok(input_tokens) = i64::try_from(input_tokens) else {
        validate_execution_plan_pricing_for_unavailable_estimate(
            state,
            plan,
            model_id,
            global_model_name,
            requested_processing_tier.as_deref(),
        )
        .await?;
        return Ok(None);
    };
    let max_output_tokens = max_output_tokens_from_request(body_json)
        .map(|value| value.saturating_mul(output_choice_count_upper_bound(&api_format, body_json)))
        .and_then(|value| i64::try_from(value).ok());
    let cache_ttl_minutes =
        aether_data_contracts::repository::usage::resolve_provider_cache_ttl_minutes(
            Some(&api_format),
            plan.model_name.as_deref(),
            global_model_name,
            Some(body_json),
        );
    if model_id.is_none() && global_model_name.is_none() {
        return Ok(None);
    }
    let cache_key = execution_plan_cost_upper_bound_cache_key(
        plan,
        model_id,
        global_model_name,
        &api_format,
        input_tokens,
        max_output_tokens,
        requested_processing_tier.as_deref(),
        cache_ttl_minutes,
    );
    let ttl = state.frontdoor_runtime_guards.auth_capacity_cache_ttl;
    if ttl.is_zero() {
        let _permit = state.acquire_auth_snapshot_load_gate().await?;
        return calculate_execution_plan_cost_upper_bound(
            state,
            plan,
            model_id,
            global_model_name,
            &api_format,
            task_type,
            input_tokens,
            max_output_tokens,
            requested_processing_tier.as_deref(),
            cache_ttl_minutes,
        )
        .await;
    }
    state
        .auth_request_cost_upper_bound_cache
        .get_or_load(cache_key, ttl, || async {
            let _permit = state.acquire_auth_snapshot_load_gate().await?;
            calculate_execution_plan_cost_upper_bound(
                state,
                plan,
                model_id,
                global_model_name,
                &api_format,
                task_type,
                input_tokens,
                max_output_tokens,
                requested_processing_tier.as_deref(),
                cache_ttl_minutes,
            )
            .await
        })
        .await
}

#[allow(clippy::too_many_arguments)]
async fn calculate_execution_plan_cost_upper_bound(
    state: &AppState,
    plan: &aether_contracts::ExecutionPlan,
    model_id: Option<&str>,
    global_model_name: Option<&str>,
    api_format: &str,
    task_type: &str,
    input_tokens: i64,
    max_output_tokens: Option<i64>,
    requested_processing_tier: Option<&str>,
    cache_ttl_minutes: Option<i64>,
) -> Result<Option<f64>, GatewayError> {
    let context =
        load_execution_plan_billing_context(state, plan, model_id, global_model_name).await?;
    let Some(context) = context else {
        return Ok(None);
    };
    let mut estimate =
        aether_billing::BillingAuthorizationEstimateInput::new(task_type, input_tokens);
    estimate.api_format = Some(api_format.to_string());
    estimate.requested_processing_tier = requested_processing_tier.map(ToOwned::to_owned);
    estimate.cache_ttl_minutes = cache_ttl_minutes;
    estimate.max_output_tokens = max_output_tokens;
    aether_billing::BillingService::new()
        .estimate_authorization_cost_upper_bound(
            &aether_billing::BillingModelPricingSnapshot::from(context),
            &estimate,
        )
        .map_err(|err| GatewayError::Internal(err.to_string()))
}

async fn validate_execution_plan_pricing_for_unavailable_estimate(
    state: &AppState,
    plan: &aether_contracts::ExecutionPlan,
    model_id: Option<&str>,
    global_model_name: Option<&str>,
    requested_processing_tier: Option<&str>,
) -> Result<(), GatewayError> {
    let started_at = std::time::Instant::now();
    let result = validate_execution_plan_pricing_for_unavailable_estimate_inner(
        state,
        plan,
        model_id,
        global_model_name,
        requested_processing_tier,
    )
    .await;
    observe_gateway_stage_ms(
        "auth_capacity_pricing_validation",
        started_at.elapsed().as_millis() as u64,
    );
    result
}

async fn validate_execution_plan_pricing_for_unavailable_estimate_inner(
    state: &AppState,
    plan: &aether_contracts::ExecutionPlan,
    model_id: Option<&str>,
    global_model_name: Option<&str>,
    requested_processing_tier: Option<&str>,
) -> Result<(), GatewayError> {
    if model_id.is_none() && global_model_name.is_none() {
        return Ok(());
    }

    let capacity_ttl = state.frontdoor_runtime_guards.auth_capacity_cache_ttl;
    if capacity_ttl.is_zero() {
        return validate_execution_plan_pricing_uncached(
            state,
            plan,
            model_id,
            global_model_name,
            requested_processing_tier,
        )
        .await;
    }
    let ttl = capacity_ttl.max(AUTH_PRICING_VALIDATION_CACHE_TTL);

    let cache_key = execution_plan_pricing_validation_cache_key(
        plan,
        model_id,
        global_model_name,
        requested_processing_tier,
    );
    let cache = state.auth_request_cost_upper_bound_cache.clone();
    cache
        .get_or_load_once_stale_while_revalidating(
            cache_key,
            ttl,
            AUTH_CAPACITY_CACHE_STALE_TTL,
            || async {
                validate_execution_plan_pricing_uncached(
                    state,
                    plan,
                    model_id,
                    global_model_name,
                    requested_processing_tier,
                )
                .await?;
                Ok::<Option<f64>, GatewayError>(Some(0.0))
            },
            || {
                let state = state.clone();
                let plan = plan.clone();
                let model_id = model_id.map(ToOwned::to_owned);
                let global_model_name = global_model_name.map(ToOwned::to_owned);
                let requested_processing_tier = requested_processing_tier.map(ToOwned::to_owned);
                async move {
                    validate_execution_plan_pricing_uncached(
                        &state,
                        &plan,
                        model_id.as_deref(),
                        global_model_name.as_deref(),
                        requested_processing_tier.as_deref(),
                    )
                    .await?;
                    Ok::<Option<f64>, GatewayError>(Some(0.0))
                }
            },
            crate::cache::CacheLoadObserver::default(),
        )
        .await?;
    Ok(())
}

async fn validate_execution_plan_pricing_uncached(
    state: &AppState,
    plan: &aether_contracts::ExecutionPlan,
    model_id: Option<&str>,
    global_model_name: Option<&str>,
    requested_processing_tier: Option<&str>,
) -> Result<(), GatewayError> {
    let _permit = state.acquire_auth_snapshot_load_gate().await?;
    let Some(context) =
        load_execution_plan_billing_context(state, plan, model_id, global_model_name).await?
    else {
        return Ok(());
    };
    aether_billing::BillingModelPricingSnapshot::from(context)
        .validate_authorization_pricing_configuration(requested_processing_tier)
        .map_err(|err| GatewayError::Internal(err.to_string()))
}

async fn load_execution_plan_billing_context(
    state: &AppState,
    plan: &aether_contracts::ExecutionPlan,
    model_id: Option<&str>,
    global_model_name: Option<&str>,
) -> Result<
    Option<aether_data_contracts::repository::billing::StoredBillingModelContext>,
    GatewayError,
> {
    let context = match model_id {
        Some(model_id) => state
            .data
            .find_billing_model_context_by_model_id(&plan.provider_id, Some(&plan.key_id), model_id)
            .await
            .map_err(|err| GatewayError::Internal(err.to_string()))?,
        None => state
            .data
            .find_billing_model_context(
                &plan.provider_id,
                Some(&plan.key_id),
                global_model_name.expect("global model name should exist"),
            )
            .await
            .map_err(|err| GatewayError::Internal(err.to_string()))?,
    };
    Ok(context)
}

fn execution_plan_cost_upper_bound_cache_key(
    plan: &aether_contracts::ExecutionPlan,
    model_id: Option<&str>,
    global_model_name: Option<&str>,
    api_format: &str,
    input_tokens: i64,
    max_output_tokens: Option<i64>,
    requested_processing_tier: Option<&str>,
    cache_ttl_minutes: Option<i64>,
) -> String {
    format!(
        "{}\x1f{}\x1f{}\x1f{}\x1f{}\x1f{}\x1f{}\x1f{}\x1f{}",
        plan.provider_id,
        plan.key_id,
        model_id.unwrap_or(""),
        global_model_name.unwrap_or(""),
        api_format,
        input_tokens,
        max_output_tokens
            .map(|value| value.to_string())
            .unwrap_or_else(|| "none".to_string()),
        requested_processing_tier.unwrap_or("standard"),
        cache_ttl_minutes
            .map(|value| value.to_string())
            .unwrap_or_else(|| "none".to_string()),
    )
}

fn execution_plan_pricing_validation_cache_key(
    plan: &aether_contracts::ExecutionPlan,
    model_id: Option<&str>,
    global_model_name: Option<&str>,
    requested_processing_tier: Option<&str>,
) -> String {
    format!(
        "pricing-validation\x1f{}\x1f{}\x1f{}\x1f{}\x1f{}",
        plan.provider_id,
        plan.key_id,
        model_id.unwrap_or(""),
        global_model_name.unwrap_or(""),
        requested_processing_tier.unwrap_or("standard"),
    )
}

fn authorization_task_type<'a>(
    api_format: &str,
    report_context: Option<&'a serde_json::Value>,
) -> Option<&'a str> {
    if report_context
        .and_then(|context| context.get("image_request"))
        .is_some()
        || api_format == "openai:image"
    {
        return None;
    }
    if api_format.ends_with(":embedding") {
        return Some("embedding");
    }
    if api_format.ends_with(":rerank") {
        return Some("rerank");
    }
    Some("chat")
}

fn report_context_string_field<'a>(
    report_context: Option<&'a serde_json::Value>,
    key: &str,
) -> Option<&'a str> {
    report_context
        .and_then(|context| context.get(key))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn max_output_tokens_from_request(value: &serde_json::Value) -> Option<u64> {
    ["max_tokens", "max_completion_tokens", "max_output_tokens"]
        .iter()
        .filter_map(|field| value.get(*field).and_then(serde_json::Value::as_u64))
        .filter(|value| *value > 0)
        .max()
}

fn output_choice_count_upper_bound(api_format: &str, value: &serde_json::Value) -> u64 {
    if api_format != "openai:chat" {
        return 1;
    }
    value
        .get("n")
        .and_then(serde_json::Value::as_u64)
        .filter(|value| *value > 0)
        .unwrap_or(1)
}

fn openai_request_input_is_self_contained(api_format: &str, value: &serde_json::Value) -> bool {
    if !api_format.starts_with("openai:") {
        return false;
    }
    let Some(object) = value.as_object() else {
        return true;
    };
    if ["previous_response_id", "conversation"]
        .iter()
        .any(|key| object.get(*key).is_some_and(has_reference_value))
    {
        return false;
    }
    if object
        .get("prompt")
        .and_then(serde_json::Value::as_object)
        .and_then(|prompt| prompt.get("id"))
        .is_some_and(has_reference_value)
    {
        return false;
    }
    !contains_indirect_request_input(value)
}

fn contains_indirect_request_input(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Array(items) => items.iter().any(contains_indirect_request_input),
        serde_json::Value::Object(object) => {
            let item_type = object
                .get("type")
                .and_then(serde_json::Value::as_str)
                .map(|value| value.trim().to_ascii_lowercase());
            if item_type.as_deref().is_some_and(|item_type| {
                matches!(
                    item_type,
                    "url"
                        | "item_reference"
                        | "input_file"
                        | "input_image"
                        | "input_audio"
                        | "image_url"
                        | "file_search"
                        | "web_search"
                        | "web_search_preview"
                        | "computer_use"
                        | "computer_use_preview"
                        | "code_interpreter"
                        | "mcp"
                        | "image_generation"
                )
            }) {
                return true;
            }
            if ["file_id", "file_uri", "fileUri", "vector_store_ids"]
                .iter()
                .any(|key| object.get(*key).is_some_and(has_reference_value))
            {
                return true;
            }
            object.values().any(contains_indirect_request_input)
        }
        _ => false,
    }
}

fn has_reference_value(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null => false,
        serde_json::Value::String(value) => !value.trim().is_empty(),
        serde_json::Value::Array(values) => !values.is_empty(),
        serde_json::Value::Object(values) => !values.is_empty(),
        _ => true,
    }
}

fn json_token_count_upper_bound(value: &serde_json::Value) -> u64 {
    serde_json::to_vec(value)
        .map(|bytes| u64::try_from(bytes.len()).unwrap_or(u64::MAX))
        .unwrap_or(u64::MAX)
}

fn model_directive_base_model_is_allowed_for_request(
    decision: &GatewayControlDecision,
    requested_model: &str,
    allowed_models: &[String],
) -> bool {
    let Some(client_api_format) = decision
        .auth_endpoint_signature
        .as_deref()
        .map(crate::ai_serving::normalize_api_format_alias)
        .filter(|value| !value.trim().is_empty())
    else {
        return false;
    };
    decision
        .model_directive_policy
        .resolve_reasoning(&client_api_format, Some(requested_model))
        .base_model()
        .is_some_and(|base_model| contains_string(allowed_models, base_model))
}

async fn request_model_resolves_to_allowed_model(
    state: &AppState,
    decision: &GatewayControlDecision,
    requested_model: &str,
    allowed_models: &[String],
) -> Result<bool, GatewayError> {
    let Some(client_api_format) = decision
        .auth_endpoint_signature
        .as_deref()
        .map(crate::ai_serving::normalize_api_format_alias)
        .filter(|value| !value.trim().is_empty())
    else {
        return Ok(false);
    };

    let resolution = decision
        .model_directive_policy
        .resolve_reasoning(&client_api_format, Some(requested_model));
    let routing_model = resolution.base_model().unwrap_or(requested_model);
    let rows = {
        let _permit = state.acquire_auth_snapshot_load_gate().await?;
        state
            .list_minimal_candidate_selection_rows_for_api_format(&client_api_format)
            .await?
    };
    let matching_rows = rows
        .into_iter()
        .filter(|row| {
            aether_scheduler_core::row_supports_requested_model_with_model_directives(
                row,
                routing_model,
                &client_api_format,
                false,
            )
        })
        .collect::<Vec<_>>();
    let Some(resolved_global_model) =
        aether_scheduler_core::resolve_requested_global_model_name_with_model_directives(
            &matching_rows,
            routing_model,
            &client_api_format,
            false,
        )
    else {
        return Ok(false);
    };
    Ok(contains_string(allowed_models, &resolved_global_model))
}
