mod fallback;
#[cfg(test)]
pub(crate) mod remote_test_support;
mod response_header_rules;
mod server;
pub(crate) mod stream;
mod stream_pump;
pub(crate) mod submission;
pub(crate) mod sync;
pub(crate) mod transport;
mod transport_failure;
pub(crate) use self::fallback::{
    analyze_local_candidate_failover_sync, local_failover_response_text,
    resolve_core_stream_direct_finalize_report_kind,
    resolve_core_stream_error_finalize_report_kind, resolve_core_sync_error_finalize_report_kind,
    resolve_local_candidate_failover_analysis_stream,
    resolve_local_candidate_failover_decision_stream, should_fallback_to_control_stream,
    should_fallback_to_control_sync, should_finalize_sync_response,
    should_retry_next_local_candidate_stream, should_retry_next_local_candidate_sync,
    should_stop_local_candidate_failover_stream, should_stop_local_candidate_failover_sync,
};
pub(crate) use self::response_header_rules::{
    apply_endpoint_response_header_rules, attach_provider_response_headers_to_report_context,
};
pub(crate) use crate::orchestration::{
    append_local_failover_policy_to_value, LocalFailoverAnalysis, LocalFailoverDecision,
};
pub(crate) use aether_gateway_execution::{
    MAX_ERROR_BODY_BYTES, MAX_STREAM_PREFETCH_BYTES, MAX_STREAM_PREFETCH_FRAMES,
};

pub(crate) fn ai_attempt_retry_scope_from_failure_disposition(
    disposition: crate::orchestration::FailureDisposition,
) -> aether_ai_serving::AiAttemptRetryScope {
    use crate::orchestration::{FailureRetryAction, FailureScope};
    use aether_ai_serving::AiAttemptRetryScope;

    match disposition.failure_scope {
        FailureScope::Credential | FailureScope::CredentialModel => AiAttemptRetryScope::Credential,
        FailureScope::Endpoint => AiAttemptRetryScope::Endpoint,
        FailureScope::Provider => AiAttemptRetryScope::Provider,
        FailureScope::None => match disposition.retry_action {
            FailureRetryAction::NextCredential => AiAttemptRetryScope::Credential,
            FailureRetryAction::NextEndpoint => AiAttemptRetryScope::Endpoint,
            FailureRetryAction::Stop
            | FailureRetryAction::SameCredential
            | FailureRetryAction::NextCandidate => AiAttemptRetryScope::Candidate,
        },
    }
}

#[cfg(test)]
mod retry_scope_tests {
    use aether_ai_serving::AiAttemptRetryScope;

    use super::ai_attempt_retry_scope_from_failure_disposition;
    use crate::orchestration::{classify_failure_disposition, LocalFailoverClassification};

    #[test]
    fn anthropic_failure_scope_survives_runtime_mapping() {
        let retry_scope = |status_code| {
            ai_attempt_retry_scope_from_failure_disposition(classify_failure_disposition(
                "claude:messages",
                LocalFailoverClassification::RetryUpstreamFailure,
                status_code,
            ))
        };

        assert_eq!(retry_scope(429), AiAttemptRetryScope::Credential);
        assert_eq!(retry_scope(500), AiAttemptRetryScope::Endpoint);
        assert_eq!(retry_scope(529), AiAttemptRetryScope::Provider);
        assert_eq!(retry_scope(400), AiAttemptRetryScope::Candidate);
    }

    #[test]
    fn non_anthropic_retry_keeps_existing_candidate_order() {
        let disposition = classify_failure_disposition(
            "openai:chat",
            LocalFailoverClassification::RetryUpstreamFailure,
            429,
        );

        assert_eq!(
            ai_attempt_retry_scope_from_failure_disposition(disposition),
            AiAttemptRetryScope::Candidate
        );
        assert!(!disposition.preserve_upstream_error);
    }
}
pub use server::{
    build_execution_runtime_router, build_execution_runtime_router_with_request_concurrency_limit,
    build_execution_runtime_router_with_request_gates, serve_execution_runtime_tcp,
    serve_execution_runtime_unix,
};
pub use transport::DirectH2cSenderPrewarmReport;

pub async fn prewarm_direct_h2c_sender_cache_from_env_for_startup(
) -> Result<Option<DirectH2cSenderPrewarmReport>, String> {
    transport::prewarm_direct_h2c_sender_cache_from_env()
        .await
        .map_err(|err| err.to_string())
}

pub(crate) use stream::{
    execute_execution_runtime_stream, execute_execution_runtime_stream_with_retry_scope,
};
pub(crate) use stream_pump::build_direct_execution_frame_stream;
pub(crate) use sync::{
    execute_execution_runtime_sync, execute_execution_runtime_sync_with_retry_scope,
};
pub(crate) use transport::execute_sync_plan_with_report_context as execute_execution_runtime_sync_plan_with_report_context;
pub(crate) use transport::{
    execute_sync_plan as execute_execution_runtime_sync_plan, DirectSyncExecutionRuntime,
    DirectUpstreamStreamExecution, ExecutionRuntimeTransportError,
};
pub(crate) use transport_failure::{
    build_transport_error_stop_response, mark_stream_candidate_watchdog_terminal_started,
    StreamCandidateWatchdogProgress,
};
