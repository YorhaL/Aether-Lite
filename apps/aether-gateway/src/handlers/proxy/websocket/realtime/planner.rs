//! Candidate planning for the public OpenAI Realtime WebSocket transport.

use axum::http::{HeaderValue, Method};
use serde_json::json;

use crate::ai_serving::{
    build_passthrough_stream_plan_from_decision, maybe_build_stream_decision_payload,
    AiExecutionDecision,
};
use crate::headers::request_origin_from_headers_and_remote_addr;
use crate::privacy::RedactionSessionSlot;
use crate::{AppState, GatewayError};

pub(super) struct PlannedRealtimeCandidate {
    pub(super) execution: AiExecutionDecision,
    pub(super) admission_plan: aether_contracts::ExecutionPlan,
    pub(super) provider_id: String,
    pub(super) endpoint_id: String,
    pub(super) key_id: String,
    pub(super) provider_model: String,
}

pub(super) async fn plan_realtime_candidate(
    state: &AppState,
    context: &crate::handlers::proxy::websocket::ingress::WebSocketRequestContext,
    client_model: &str,
) -> Result<Option<PlannedRealtimeCandidate>, GatewayError> {
    let parts = realtime_planning_parts(context);
    let body = json!({"model": client_model});
    let Some(execution) = maybe_build_stream_decision_payload(
        state,
        &parts,
        context.trace_id.as_str(),
        &context.decision,
        &body,
        None,
    )
    .await?
    else {
        return Ok(None);
    };
    if execution
        .provider_api_format
        .as_deref()
        .map(crate::ai_serving::normalize_api_format_alias)
        .as_deref()
        != Some("openai:realtime")
    {
        return Ok(None);
    }

    let Some(attempt) = build_passthrough_stream_plan_from_decision(&parts, execution.clone())?
    else {
        return Ok(None);
    };
    let provider_id = execution.provider_id.clone().unwrap_or_default();
    let endpoint_id = execution.endpoint_id.clone().unwrap_or_default();
    let key_id = execution.key_id.clone().unwrap_or_default();
    let provider_model = execution
        .mapped_model
        .clone()
        .or_else(|| execution.model_name.clone())
        .unwrap_or_default();
    if provider_id.is_empty()
        || endpoint_id.is_empty()
        || key_id.is_empty()
        || provider_model.trim().is_empty()
        || execution.upstream_url.as_deref().is_none_or(str::is_empty)
    {
        return Ok(None);
    }

    Ok(Some(PlannedRealtimeCandidate {
        execution,
        admission_plan: attempt.plan,
        provider_id,
        endpoint_id,
        key_id,
        provider_model,
    }))
}

fn realtime_planning_parts(
    context: &crate::handlers::proxy::websocket::ingress::WebSocketRequestContext,
) -> http::request::Parts {
    let mut request = http::Request::builder()
        .method(Method::GET)
        .uri(context.uri.clone())
        .body(())
        .expect("the authenticated Realtime URI must remain valid");
    *request.headers_mut() = context.headers.clone();
    request.headers_mut().insert(
        http::header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    request
        .extensions_mut()
        .insert(request_origin_from_headers_and_remote_addr(
            &context.headers,
            &context.remote_addr,
        ));
    request
        .extensions_mut()
        .insert(RedactionSessionSlot::default());
    request.into_parts().0
}
