pub(crate) mod api;
mod planner;
mod pure;
pub(crate) mod transport;

use axum::http::Uri;

use crate::{AppState, GatewayError};

pub(crate) use self::planner::{
    apply_local_runtime_candidate_terminal_reason, build_local_same_format_stream_attempt_source,
    build_local_same_format_stream_plan_and_reports, build_local_same_format_sync_attempt_source,
    build_local_same_format_sync_plan_and_reports, maybe_build_stream_decision_payload,
    maybe_build_stream_plan_payload, maybe_build_sync_decision_payload,
    maybe_build_sync_plan_payload, planner_is_matching_stream_request,
    read_candidate_transport_snapshot, record_local_runtime_candidate_skip_reason,
    CandidateFailureDiagnostic, CandidateFailureDiagnosticKind, EligibleLocalExecutionCandidate,
    GatewayAuthApiKeySnapshot, GatewayProviderTransportSnapshot, LocalExecutionAttemptSource,
    PlannerAppState, SkippedLocalExecutionCandidate,
};
pub(crate) use self::pure::*;
pub(crate) use self::transport::{
    append_transport_diagnostics_to_value, candidate_common_transport_skip_reason,
    candidate_transport_pair_skip_reason, CandidateTransportPolicyFacts,
};
pub(crate) use crate::control::{GatewayControlDecision, GatewayCredentialCarrier};
pub(crate) use crate::headers::RequestOrigin;
pub(crate) use aether_ai_serving::{
    augment_sync_report_context,
    build_ai_report_context_original_request_echo as build_report_context_original_request_echo,
    extract_ai_gemini_model_from_path as extract_gemini_model_from_path,
    generic_decision_missing_exact_provider_request as generic_decision_missing_exact_provider_request_impl,
    AiExecutionDecision, AiExecutionPlanPayload, AiStreamAttempt, AiSyncAttempt,
};

pub(crate) async fn resolve_execution_runtime_auth_context(
    state: &AppState,
    decision: &GatewayControlDecision,
    headers: &http::HeaderMap,
    uri: &Uri,
    trace_id: &str,
) -> Result<Option<crate::control::GatewayControlAuthContext>, GatewayError> {
    crate::control::resolve_execution_runtime_auth_context(state, decision, headers, uri, trace_id)
        .await
}

pub(crate) fn collect_control_headers(
    headers: &http::HeaderMap,
) -> std::collections::BTreeMap<String, String> {
    crate::headers::collect_control_headers(headers)
}

pub(crate) fn request_origin_from_headers(headers: &http::HeaderMap) -> RequestOrigin {
    crate::headers::request_origin_from_headers(headers)
}

pub(crate) fn request_origin_from_parts(parts: &http::request::Parts) -> RequestOrigin {
    crate::headers::request_origin_from_parts(parts)
}

pub(crate) fn is_json_request(headers: &http::HeaderMap) -> bool {
    crate::headers::is_json_request(headers)
}

pub(crate) fn decoded_request_body_bytes<'a>(
    headers: &http::HeaderMap,
    body_bytes: &'a [u8],
) -> Result<std::borrow::Cow<'a, [u8]>, crate::headers::RequestBodyNormalizationError> {
    crate::headers::decoded_request_body_bytes(headers, body_bytes)
}

pub(crate) fn tls_fingerprint_from_headers(headers: &http::HeaderMap) -> Option<serde_json::Value> {
    crate::headers::tls_fingerprint_from_headers(headers)
}

pub(crate) fn build_execution_runtime_auth_context(
    auth_context: &crate::control::GatewayControlAuthContext,
) -> ExecutionRuntimeAuthContext {
    ExecutionRuntimeAuthContext {
        user_id: auth_context.user_id.clone(),
        api_key_id: auth_context.api_key_id.clone(),
        username: auth_context.username.clone(),
        api_key_name: auth_context.api_key_name.clone(),
        balance_remaining: auth_context.balance_remaining,
        access_allowed: auth_context.access_allowed,
        api_key_is_standalone: auth_context.api_key_is_standalone,
    }
}

pub(crate) fn resolve_decision_execution_runtime_auth_context(
    decision: &GatewayControlDecision,
) -> Option<ExecutionRuntimeAuthContext> {
    decision
        .auth_context
        .as_ref()
        .map(build_execution_runtime_auth_context)
}

pub(crate) fn resolve_local_decision_execution_runtime_auth_context(
    decision: &GatewayControlDecision,
) -> Option<ExecutionRuntimeAuthContext> {
    resolve_decision_execution_runtime_auth_context(decision).filter(|auth_context| {
        auth_context.access_allowed
            && !auth_context.user_id.trim().is_empty()
            && !auth_context.api_key_id.trim().is_empty()
    })
}

pub(crate) fn generic_decision_missing_exact_provider_request(
    payload: &AiExecutionDecision,
) -> bool {
    if !generic_decision_missing_exact_provider_request_impl(payload) {
        return false;
    }

    tracing::warn!(
        decision_kind = payload.decision_kind.as_deref().unwrap_or_default(),
        provider_api_format = payload.provider_api_format.as_deref().unwrap_or_default(),
        client_api_format = payload.client_api_format.as_deref().unwrap_or_default(),
        "gateway generic decision missing exact provider request; falling back to plan"
    );
    true
}
