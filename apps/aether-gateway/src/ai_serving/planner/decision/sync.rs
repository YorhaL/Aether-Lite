use crate::ai_serving::planner::route::resolve_execution_runtime_sync_plan_kind;
use crate::ai_serving::GatewayControlDecision;
use crate::{AiExecutionDecision, AppState, GatewayError};

pub(crate) async fn maybe_build_sync_decision_payload(
    state: &AppState,
    parts: &http::request::Parts,
    trace_id: &str,
    decision: &GatewayControlDecision,
    body_json: &serde_json::Value,
    _body_base64: Option<&str>,
    _body_is_empty: bool,
) -> Result<Option<AiExecutionDecision>, GatewayError> {
    let Some(plan_kind) = resolve_execution_runtime_sync_plan_kind(parts, decision) else {
        return Ok(None);
    };

    super::maybe_build_sync_local_same_format_provider_decision_payload(
        state, parts, trace_id, decision, body_json, plan_kind,
    )
    .await
}
