use aether_ai_serving::{
    run_ai_stream_execution_path, AiPlanFallbackReason, AiServingExecutionOutcome,
    AiStreamExecutionPathPort, AiStreamExecutionStep, OriginalRequestPayload,
};
use async_trait::async_trait;
use axum::body::{Body, Bytes};
use axum::http::Response;

use crate::ai_serving::api::{
    is_matching_stream_request, resolve_execution_runtime_stream_plan_kind,
    supports_stream_execution_decision_kind,
};
use crate::control::GatewayControlDecision;
use crate::{AppState, GatewayError, GatewayFallbackReason};

use super::{
    maybe_execute_stream_via_local_same_format_provider_decision,
    maybe_execute_stream_via_plan_fallback, maybe_execute_stream_via_remote_decision,
    parse_local_request_body, CandidateExecutionContext, LocalExecutionRequestOutcome,
};

pub(crate) async fn maybe_execute_via_stream_decision_path(
    state: &AppState,
    parts: &http::request::Parts,
    body_bytes: &Bytes,
    trace_id: &str,
    decision: &GatewayControlDecision,
) -> Result<LocalExecutionRequestOutcome, GatewayError> {
    let Some(plan_kind) = resolve_execution_runtime_stream_plan_kind(parts, decision) else {
        return Ok(LocalExecutionRequestOutcome::NoPath);
    };
    let Some((body_json, body_base64)) = parse_local_request_body(parts, body_bytes) else {
        return Ok(LocalExecutionRequestOutcome::NoPath);
    };

    let mut planning_parts = parts.clone();
    if crate::ai_serving::is_json_request(&planning_parts.headers) {
        if let Ok(decoded_body) = crate::ai_serving::decoded_request_body_bytes(
            &planning_parts.headers,
            body_bytes.as_ref(),
        ) {
            planning_parts
                .extensions
                .insert(OriginalRequestPayload::from_parsed_json(
                    body_json.clone(),
                    decoded_body.as_ref(),
                ));
        }
    }
    let parts = &planning_parts;

    if !is_matching_stream_request(plan_kind, parts, &body_json, body_base64.as_deref()) {
        return Ok(LocalExecutionRequestOutcome::NoPath);
    }

    let port = GatewayStreamExecutionPathPort {
        state,
        parts,
        trace_id,
        decision,
        body_json: &body_json,
        body_base64,
        plan_kind,
        scheduler_supported: supports_stream_execution_decision_kind(plan_kind),
        execution_context: CandidateExecutionContext::default(),
    };

    Ok(from_ai_serving_outcome(
        run_ai_stream_execution_path(&port).await?,
    ))
}

struct GatewayStreamExecutionPathPort<'a> {
    state: &'a AppState,
    parts: &'a http::request::Parts,
    trace_id: &'a str,
    decision: &'a GatewayControlDecision,
    body_json: &'a serde_json::Value,
    body_base64: Option<String>,
    plan_kind: &'a str,
    scheduler_supported: bool,
    execution_context: CandidateExecutionContext,
}

#[async_trait]
impl AiStreamExecutionPathPort for GatewayStreamExecutionPathPort<'_> {
    type Response = Response<Body>;
    type Exhaustion = super::LocalExecutionExhaustion;
    type Error = GatewayError;

    fn scheduler_decision_supported(&self) -> bool {
        self.scheduler_supported
    }

    async fn execute_stream_step(
        &self,
        step: AiStreamExecutionStep,
    ) -> Result<AiServingExecutionOutcome<Self::Response, Self::Exhaustion>, Self::Error> {
        let outcome = match step {
            AiStreamExecutionStep::LocalSameFormatProvider => {
                maybe_execute_stream_via_local_same_format_provider_decision(
                    self.state,
                    self.parts,
                    self.trace_id,
                    self.decision,
                    self.body_json,
                    self.plan_kind,
                    &self.execution_context,
                )
                .await?
            }
            AiStreamExecutionStep::RemoteDecision => {
                if let Some(response) = maybe_execute_stream_via_remote_decision(
                    self.state,
                    self.parts,
                    self.trace_id,
                    self.decision,
                    self.body_json,
                    self.plan_kind,
                )
                .await?
                {
                    LocalExecutionRequestOutcome::Responded(response)
                } else {
                    LocalExecutionRequestOutcome::NoPath
                }
            }
        };
        Ok(to_ai_serving_outcome(outcome))
    }

    async fn execute_stream_plan_fallback(
        &self,
        reason: AiPlanFallbackReason,
    ) -> Result<AiServingExecutionOutcome<Self::Response, Self::Exhaustion>, Self::Error> {
        let outcome = maybe_execute_stream_via_plan_fallback(
            self.state,
            self.parts,
            self.trace_id,
            self.decision,
            self.body_json,
            self.body_base64.clone(),
            self.plan_kind,
            gateway_fallback_reason(reason),
            &self.execution_context,
        )
        .await?;
        Ok(to_ai_serving_outcome(outcome))
    }
}

fn to_ai_serving_outcome(
    outcome: LocalExecutionRequestOutcome,
) -> AiServingExecutionOutcome<Response<Body>, super::LocalExecutionExhaustion> {
    match outcome {
        LocalExecutionRequestOutcome::Responded(response) => {
            if super::is_deferred_upstream_response(&response) {
                AiServingExecutionOutcome::Deferred(response)
            } else {
                AiServingExecutionOutcome::Responded(response)
            }
        }
        LocalExecutionRequestOutcome::Exhausted(outcome) => {
            AiServingExecutionOutcome::Exhausted(outcome)
        }
        LocalExecutionRequestOutcome::NoPath => AiServingExecutionOutcome::NoPath,
    }
}

fn from_ai_serving_outcome(
    outcome: AiServingExecutionOutcome<Response<Body>, super::LocalExecutionExhaustion>,
) -> LocalExecutionRequestOutcome {
    match outcome {
        AiServingExecutionOutcome::Responded(response)
        | AiServingExecutionOutcome::Deferred(response) => {
            LocalExecutionRequestOutcome::Responded(response)
        }
        AiServingExecutionOutcome::Exhausted(outcome) => {
            LocalExecutionRequestOutcome::Exhausted(outcome)
        }
        AiServingExecutionOutcome::NoPath => LocalExecutionRequestOutcome::NoPath,
    }
}

fn gateway_fallback_reason(reason: AiPlanFallbackReason) -> GatewayFallbackReason {
    match reason {
        AiPlanFallbackReason::RemoteDecisionMiss => GatewayFallbackReason::RemoteDecisionMiss,
        AiPlanFallbackReason::SchedulerDecisionUnsupported => {
            GatewayFallbackReason::SchedulerDecisionUnsupported
        }
    }
}
