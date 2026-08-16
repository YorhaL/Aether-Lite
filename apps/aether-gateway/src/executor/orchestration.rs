use std::future::Future;
use std::pin::Pin;

use crate::ai_serving::api::{
    build_local_same_format_stream_attempt_source, build_local_same_format_sync_attempt_source,
    parse_direct_request_body, resolve_local_same_format_stream_spec,
    resolve_local_same_format_sync_spec, AiStreamAttempt, AiSyncAttempt,
    EXECUTION_RUNTIME_STREAM_DECISION_ACTION, EXECUTION_RUNTIME_SYNC_DECISION_ACTION,
};
use crate::control::GatewayControlDecision;
use crate::executor::candidate_loop::{
    execute_stream_attempt_source_with_execution_context,
    execute_sync_attempt_source_with_execution_context, CandidateExecutionContext,
};
use crate::{AiExecutionDecision, AppState, GatewayError};

use super::LocalExecutionRequestOutcome;

pub(crate) async fn maybe_execute_sync_local_path(
    state: &AppState,
    parts: &http::request::Parts,
    body_bytes: &axum::body::Bytes,
    trace_id: &str,
    decision: &GatewayControlDecision,
) -> Result<LocalExecutionRequestOutcome, GatewayError> {
    super::maybe_execute_via_sync_decision_path(state, parts, body_bytes, trace_id, decision).await
}

pub(crate) async fn maybe_execute_stream_local_path(
    state: &AppState,
    parts: &http::request::Parts,
    body_bytes: &axum::body::Bytes,
    trace_id: &str,
    decision: &GatewayControlDecision,
) -> Result<LocalExecutionRequestOutcome, GatewayError> {
    super::maybe_execute_via_stream_decision_path(state, parts, body_bytes, trace_id, decision)
        .await
}

pub(crate) async fn maybe_execute_sync_via_local_same_format_provider_decision(
    state: &AppState,
    parts: &http::request::Parts,
    trace_id: &str,
    decision: &GatewayControlDecision,
    body_json: &serde_json::Value,
    plan_kind: &str,
    execution_context: &CandidateExecutionContext,
) -> Result<LocalExecutionRequestOutcome, GatewayError> {
    let Some(spec) = resolve_local_same_format_sync_spec(plan_kind) else {
        return Ok(LocalExecutionRequestOutcome::NoPath);
    };
    let Some((attempt_source, _candidate_count)) = build_local_same_format_sync_attempt_source(
        state, parts, trace_id, decision, body_json, spec,
    )
    .await?
    else {
        return Ok(LocalExecutionRequestOutcome::NoPath);
    };

    execute_sync_attempt_source_with_execution_context::<AiSyncAttempt, _>(
        state,
        parts,
        trace_id,
        decision,
        plan_kind,
        attempt_source,
        execution_context,
    )
    .await
}

pub(crate) async fn maybe_execute_stream_via_local_same_format_provider_decision(
    state: &AppState,
    parts: &http::request::Parts,
    trace_id: &str,
    decision: &GatewayControlDecision,
    body_json: &serde_json::Value,
    plan_kind: &str,
    execution_context: &CandidateExecutionContext,
) -> Result<LocalExecutionRequestOutcome, GatewayError> {
    let Some(spec) = resolve_local_same_format_stream_spec(plan_kind) else {
        return Ok(LocalExecutionRequestOutcome::NoPath);
    };
    let Some((attempt_source, _candidate_count)) = build_local_same_format_stream_attempt_source(
        state, parts, trace_id, decision, body_json, spec,
    )
    .await?
    else {
        return Ok(LocalExecutionRequestOutcome::NoPath);
    };

    execute_stream_attempt_source_with_execution_context::<AiStreamAttempt, _>(
        state,
        trace_id,
        decision,
        plan_kind,
        attempt_source,
        execution_context,
    )
    .await
}

pub(crate) fn maybe_execute_sync_request<'a>(
    state: &'a AppState,
    parts: &'a http::request::Parts,
    body_bytes: &'a axum::body::Bytes,
    trace_id: &'a str,
    decision: Option<&'a GatewayControlDecision>,
) -> Pin<Box<dyn Future<Output = Result<LocalExecutionRequestOutcome, GatewayError>> + Send + 'a>> {
    Box::pin(async move {
        let Some(decision) = decision else {
            return Ok(LocalExecutionRequestOutcome::NoPath);
        };
        if parts.method != http::Method::POST {
            return Ok(LocalExecutionRequestOutcome::NoPath);
        }
        maybe_execute_sync_local_path(state, parts, body_bytes, trace_id, decision).await
    })
}

pub(crate) fn maybe_execute_stream_request<'a>(
    state: &'a AppState,
    parts: &'a http::request::Parts,
    body_bytes: &'a axum::body::Bytes,
    trace_id: &'a str,
    decision: Option<&'a GatewayControlDecision>,
) -> Pin<Box<dyn Future<Output = Result<LocalExecutionRequestOutcome, GatewayError>> + Send + 'a>> {
    Box::pin(async move {
        let Some(decision) = decision else {
            return Ok(LocalExecutionRequestOutcome::NoPath);
        };
        if parts.method != http::Method::POST {
            return Ok(LocalExecutionRequestOutcome::NoPath);
        }
        maybe_execute_stream_local_path(state, parts, body_bytes, trace_id, decision).await
    })
}

pub(crate) fn planner_decision_action(action: &str) -> bool {
    matches!(
        action,
        EXECUTION_RUNTIME_SYNC_DECISION_ACTION | EXECUTION_RUNTIME_STREAM_DECISION_ACTION
    )
}

pub(crate) fn parse_local_request_body(
    parts: &http::request::Parts,
    body_bytes: &axum::body::Bytes,
) -> Option<(serde_json::Value, Option<String>)> {
    parse_direct_request_body(parts, body_bytes)
}

pub(crate) fn decision_payload_is_direct_execution(payload: &AiExecutionDecision) -> bool {
    planner_decision_action(payload.action.as_str())
}
