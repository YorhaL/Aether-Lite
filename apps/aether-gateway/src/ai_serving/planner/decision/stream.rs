use crate::ai_serving::planner::route::{
    is_matching_stream_request, resolve_execution_runtime_stream_plan_kind,
};
use crate::ai_serving::GatewayControlDecision;
use crate::{AiExecutionDecision, AppState, GatewayError};

pub(crate) async fn maybe_build_stream_decision_payload(
    state: &AppState,
    parts: &http::request::Parts,
    trace_id: &str,
    decision: &GatewayControlDecision,
    body_json: &serde_json::Value,
    body_base64: Option<&str>,
) -> Result<Option<AiExecutionDecision>, GatewayError> {
    let Some(plan_kind) = resolve_execution_runtime_stream_plan_kind(parts, decision) else {
        return Ok(None);
    };
    if !is_matching_stream_request(plan_kind, parts, body_json, body_base64) {
        return Ok(None);
    }

    super::maybe_build_stream_local_same_format_provider_decision_payload(
        state, parts, trace_id, decision, body_json, plan_kind,
    )
    .await
}
