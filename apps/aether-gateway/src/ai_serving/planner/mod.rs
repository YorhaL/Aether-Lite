use crate::ai_serving::{AiExecutionDecision, AiExecutionPlanPayload, GatewayControlDecision};
use crate::{AppState, GatewayError};

mod candidate_affinity_cache;
mod candidate_materialization;
mod candidate_metadata;
mod candidate_preparation;
mod candidate_ranking;
mod candidate_resolution;
mod candidate_source;
mod common;
mod decision;
mod decision_input;
mod materialization_policy;
mod passthrough;
mod plan_builders;
mod redaction;
mod report_context;
mod request_gzip;
mod route;
mod runtime_miss;
mod spec_metadata;
mod state;

pub(crate) use self::candidate_materialization::LocalExecutionAttemptSource;
pub(crate) use self::candidate_resolution::{
    read_candidate_transport_snapshot, EligibleLocalExecutionCandidate,
    SkippedLocalExecutionCandidate,
};
pub(crate) use self::passthrough::{
    build_local_same_format_stream_attempt_source, build_local_same_format_stream_plan_and_reports,
    build_local_same_format_sync_attempt_source, build_local_same_format_sync_plan_and_reports,
};
pub(crate) use self::plan_builders::{AiStreamAttempt, AiSyncAttempt};
pub(crate) use self::request_gzip::resolve_transport_request_encoding_policy;
pub(crate) use self::route::is_matching_stream_request as planner_is_matching_stream_request;
pub(crate) use self::runtime_miss::{
    apply_local_runtime_candidate_terminal_reason, record_local_runtime_candidate_skip_reason,
};
pub(crate) use self::state::{
    GatewayAuthApiKeySnapshot, GatewayProviderTransportSnapshot, PlannerAppState,
};
pub(crate) use aether_ai_serving::{
    build_ai_execution_decision_response, AiExecutionDecisionResponseParts,
    CandidateFailureDiagnostic, CandidateFailureDiagnosticKind,
};

pub(crate) async fn maybe_build_sync_decision_payload(
    state: &AppState,
    parts: &http::request::Parts,
    trace_id: &str,
    decision: &GatewayControlDecision,
    body_json: &serde_json::Value,
    body_base64: Option<&str>,
    body_is_empty: bool,
) -> Result<Option<AiExecutionDecision>, GatewayError> {
    decision::maybe_build_sync_decision_payload(
        state,
        parts,
        trace_id,
        decision,
        body_json,
        body_base64,
        body_is_empty,
    )
    .await
}

pub(crate) async fn maybe_build_stream_decision_payload(
    state: &AppState,
    parts: &http::request::Parts,
    trace_id: &str,
    decision: &GatewayControlDecision,
    body_json: &serde_json::Value,
    body_base64: Option<&str>,
) -> Result<Option<AiExecutionDecision>, GatewayError> {
    decision::maybe_build_stream_decision_payload(
        state,
        parts,
        trace_id,
        decision,
        body_json,
        body_base64,
    )
    .await
}

pub(crate) async fn maybe_build_sync_plan_payload(
    state: &AppState,
    parts: &http::request::Parts,
    trace_id: &str,
    decision: &GatewayControlDecision,
    body_json: &serde_json::Value,
    body_base64: Option<&str>,
    body_is_empty: bool,
) -> Result<Option<AiExecutionPlanPayload>, GatewayError> {
    decision::maybe_build_sync_plan_payload_impl(
        state,
        parts,
        trace_id,
        decision,
        body_json,
        body_base64,
        body_is_empty,
    )
    .await
}

pub(crate) async fn maybe_build_stream_plan_payload(
    state: &AppState,
    parts: &http::request::Parts,
    trace_id: &str,
    decision: &GatewayControlDecision,
    body_json: &serde_json::Value,
    body_base64: Option<&str>,
) -> Result<Option<AiExecutionPlanPayload>, GatewayError> {
    decision::maybe_build_stream_plan_payload_impl(
        state,
        parts,
        trace_id,
        decision,
        body_json,
        body_base64,
    )
    .await
}
