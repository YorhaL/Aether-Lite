use crate::ai_serving::planner::plan_builders::{
    build_passthrough_stream_plan_from_decision, build_passthrough_sync_plan_from_decision,
    AiStreamAttempt, AiSyncAttempt,
};
use crate::ai_serving::AiExecutionDecision;
use crate::GatewayError;

pub(crate) use aether_ai_serving::{
    ai_openai_image_spec_metadata as local_openai_image_spec_metadata,
    ai_openai_responses_spec_metadata as local_openai_responses_spec_metadata,
    ai_requested_model_family_for_same_format_provider as requested_model_family_for_same_format_provider,
    ai_requested_model_family_for_standard_source as requested_model_family_for_standard_source,
    ai_same_format_provider_spec_metadata as local_same_format_provider_spec_metadata,
    ai_standard_spec_metadata as local_standard_spec_metadata,
    AiExecutionSurfaceSpecMetadata as LocalExecutionSurfaceSpecMetadata,
    AiRequestedModelFamily as RequestedModelFamily,
};

pub(crate) fn build_sync_plan_from_requested_model_family(
    _family: RequestedModelFamily,
    parts: &http::request::Parts,
    _body_json: &serde_json::Value,
    payload: AiExecutionDecision,
) -> Result<Option<AiSyncAttempt>, GatewayError> {
    build_passthrough_sync_plan_from_decision(parts, payload)
}

pub(crate) fn build_stream_plan_from_requested_model_family(
    _family: RequestedModelFamily,
    parts: &http::request::Parts,
    _body_json: &serde_json::Value,
    payload: AiExecutionDecision,
) -> Result<Option<AiStreamAttempt>, GatewayError> {
    build_passthrough_stream_plan_from_decision(parts, payload)
}
