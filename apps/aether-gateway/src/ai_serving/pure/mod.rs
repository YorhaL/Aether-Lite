pub(crate) use crate::ai_serving::{
    build_local_same_format_stream_attempt_source, build_local_same_format_stream_plan_and_reports,
    build_local_same_format_sync_attempt_source, build_local_same_format_sync_plan_and_reports,
    maybe_build_stream_decision_payload, maybe_build_stream_plan_payload,
    maybe_build_sync_decision_payload, maybe_build_sync_plan_payload, AiExecutionDecision,
    AiExecutionPlanPayload, AiStreamAttempt, AiSyncAttempt,
};
pub(crate) use aether_ai_formats::api::*;
pub(crate) use aether_ai_formats::{
    api_format_defaults_to_client_error_failover, api_format_defaults_to_non_stream,
    is_embedding_api_format, is_rerank_api_format,
};

pub(crate) fn plan_kind_matches_api_operation(
    plan_kind: &str,
    require_streaming: bool,
    expected_operation: Option<ApiOperation>,
) -> bool {
    let Some(expected_operation) = expected_operation else {
        return true;
    };
    if expected_operation == ApiOperation::OpenAiResponsesCompact {
        return if require_streaming {
            plan_kind == OPENAI_RESPONSES_COMPACT_STREAM_PLAN_KIND
        } else {
            plan_kind == OPENAI_RESPONSES_COMPACT_SYNC_PLAN_KIND
        };
    }
    let resolved_operation = if require_streaming {
        resolve_local_same_format_stream_spec(plan_kind).and_then(|spec| spec.operation)
    } else {
        resolve_local_same_format_sync_spec(plan_kind).and_then(|spec| spec.operation)
    };
    resolved_operation == Some(expected_operation)
}
