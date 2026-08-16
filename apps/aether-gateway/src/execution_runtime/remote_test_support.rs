use aether_contracts::{ExecutionPlan, ExecutionResult};

use crate::constants::TRACE_ID_HEADER;
use crate::{AppState, GatewayError};

fn build_remote_execution_runtime_request(
    state: &AppState,
    remote_execution_runtime_base_url: &str,
    path: &str,
    trace_id: Option<&str>,
    plan: &ExecutionPlan,
) -> reqwest::RequestBuilder {
    let mut request = state
        .client
        .post(format!("{remote_execution_runtime_base_url}{path}"))
        .json(plan);
    if let Some(trace_id) = trace_id.map(str::trim).filter(|value| !value.is_empty()) {
        request = request.header(TRACE_ID_HEADER, trace_id);
    }
    request
}

pub(crate) async fn post_sync_plan_to_remote_execution_runtime(
    state: &AppState,
    remote_execution_runtime_base_url: &str,
    trace_id: Option<&str>,
    plan: &ExecutionPlan,
) -> Result<reqwest::Response, GatewayError> {
    build_remote_execution_runtime_request(
        state,
        remote_execution_runtime_base_url,
        "/v1/execute/sync",
        trace_id,
        plan,
    )
    .send()
    .await
    .map_err(|err| GatewayError::Internal(err.to_string()))
}

pub(crate) async fn post_stream_plan_to_remote_execution_runtime(
    state: &AppState,
    remote_execution_runtime_base_url: &str,
    trace_id: Option<&str>,
    plan: &ExecutionPlan,
) -> Result<reqwest::Response, GatewayError> {
    build_remote_execution_runtime_request(
        state,
        remote_execution_runtime_base_url,
        "/v1/execute/stream",
        trace_id,
        plan,
    )
    .send()
    .await
    .map_err(|err| GatewayError::Internal(err.to_string()))
}

pub(crate) async fn execute_sync_plan_via_remote_execution_runtime(
    state: &AppState,
    remote_execution_runtime_base_url: &str,
    trace_id: Option<&str>,
    plan: &ExecutionPlan,
) -> Result<ExecutionResult, GatewayError> {
    let response = post_sync_plan_to_remote_execution_runtime(
        state,
        remote_execution_runtime_base_url,
        trace_id,
        plan,
    )
    .await?;
    if response.status() != http::StatusCode::OK {
        return Err(GatewayError::Internal(format!(
            "execution runtime returned HTTP {}",
            response.status()
        )));
    }

    response
        .json::<ExecutionResult>()
        .await
        .map_err(|err| GatewayError::Internal(err.to_string()))
}
