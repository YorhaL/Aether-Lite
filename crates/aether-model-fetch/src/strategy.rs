use std::collections::BTreeSet;

use aether_contracts::ExecutionResult;
use aether_provider_transport::GatewayProviderTransportSnapshot;
use serde_json::Value;

use crate::logic::{aggregate_models_for_cache, parse_models_response_page};
use crate::transport::{build_standard_models_fetch_execution_plan, ModelFetchTransportRuntime};

#[derive(Debug, Clone, PartialEq)]
pub struct ModelsFetchOutcome {
    pub fetched_model_ids: Vec<String>,
    pub cached_models: Vec<Value>,
    pub errors: Vec<String>,
    pub has_success: bool,
}

pub async fn fetch_models_from_transports(
    runtime: &(impl ModelFetchTransportRuntime + ?Sized),
    transports: &[GatewayProviderTransportSnapshot],
) -> Result<ModelsFetchOutcome, String> {
    if transports.is_empty() {
        return Err("No transport snapshots available for models fetch".to_string());
    }

    let mut all_models = Vec::new();
    let mut errors = Vec::new();
    let mut has_success = false;
    for transport in transports {
        match fetch_models_from_transport(runtime, transport).await {
            Ok(models) => {
                all_models.extend(models);
                has_success = true;
            }
            Err(error) => errors.push(format!("{}: {error}", transport.endpoint.api_format.trim())),
        }
    }

    let cached_models = aggregate_models_for_cache(&all_models);
    let fetched_model_ids = cached_models
        .iter()
        .filter_map(|model| model.get("id"))
        .filter_map(Value::as_str)
        .map(ToOwned::to_owned)
        .collect();
    Ok(ModelsFetchOutcome {
        fetched_model_ids,
        cached_models,
        errors,
        has_success,
    })
}

async fn fetch_models_from_transport(
    runtime: &(impl ModelFetchTransportRuntime + ?Sized),
    transport: &GatewayProviderTransportSnapshot,
) -> Result<Vec<Value>, String> {
    let mut models = Vec::new();
    let mut seen_ids = BTreeSet::new();
    let mut next_after_id = None;

    for _ in 0..20 {
        let plan = build_standard_models_fetch_execution_plan(transport, next_after_id.as_deref())?;
        let result = runtime.execute_model_fetch_execution_plan(&plan).await?;
        let body = execution_result_json_body(&result)?;
        let parsed = parse_models_response_page(&transport.endpoint.api_format, &body)?;
        for model in parsed.cached_models {
            let Some(model_id) = model
                .get("id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            else {
                continue;
            };
            if seen_ids.insert(model_id.to_string()) {
                models.push(model);
            }
        }

        let Some(next_cursor) = parsed
            .has_more
            .then_some(parsed.next_after_id)
            .flatten()
            .filter(|value| next_after_id.as_deref() != Some(value.as_str()))
        else {
            break;
        };
        next_after_id = Some(next_cursor);
    }

    Ok(models)
}

fn execution_result_json_body(result: &ExecutionResult) -> Result<Value, String> {
    if !(200..300).contains(&result.status_code) {
        return Err(result
            .error
            .as_ref()
            .map(|error| error.message.clone())
            .unwrap_or_else(|| format!("upstream returned HTTP {}", result.status_code)));
    }
    result
        .body
        .as_ref()
        .and_then(|body| body.json_body.clone())
        .ok_or_else(|| "models response is not JSON".to_string())
}
