use aether_contracts::{ExecutionPlan, ExecutionResult};
use serde_json::Value;

use crate::orchestration::{
    resolve_local_failover_analysis_for_attempt, LocalFailoverAnalysis,
    LocalFailoverClassification, LocalFailoverDecision,
};
use crate::AppState;

fn openai_image_success_disables_local_success_failover(
    plan: &ExecutionPlan,
    status_code: u16,
) -> bool {
    status_code == 200
        && plan
            .provider_api_format
            .eq_ignore_ascii_case("openai:image")
}

pub(crate) async fn should_retry_next_local_candidate_sync(
    state: &AppState,
    plan: &ExecutionPlan,
    plan_kind: &str,
    report_context: Option<&serde_json::Value>,
    result: &ExecutionResult,
    response_text: Option<&str>,
) -> bool {
    matches!(
        analyze_local_candidate_failover_sync(
            state,
            plan,
            plan_kind,
            report_context,
            result,
            response_text,
        )
        .await
        .decision,
        LocalFailoverDecision::RetryNextCandidate
    )
}

pub(crate) async fn analyze_local_candidate_failover_sync(
    state: &AppState,
    plan: &ExecutionPlan,
    _plan_kind: &str,
    report_context: Option<&serde_json::Value>,
    result: &ExecutionResult,
    response_text: Option<&str>,
) -> LocalFailoverAnalysis {
    if openai_image_success_disables_local_success_failover(plan, result.status_code) {
        return LocalFailoverAnalysis::use_default();
    }

    if let Some(error) = result.error.as_ref() {
        if !error.retryable && !error.failover_recommended {
            return LocalFailoverAnalysis {
                classification: LocalFailoverClassification::StopExecutionError,
                decision: LocalFailoverDecision::StopLocalFailover,
            };
        }
    }

    resolve_local_failover_analysis_for_attempt(
        state,
        plan,
        report_context,
        result.status_code,
        response_text,
    )
    .await
}

pub(crate) async fn should_stop_local_candidate_failover_sync(
    state: &AppState,
    plan: &ExecutionPlan,
    plan_kind: &str,
    report_context: Option<&serde_json::Value>,
    result: &ExecutionResult,
    response_text: Option<&str>,
) -> bool {
    matches!(
        analyze_local_candidate_failover_sync(
            state,
            plan,
            plan_kind,
            report_context,
            result,
            response_text,
        )
        .await,
        LocalFailoverAnalysis {
            decision: LocalFailoverDecision::StopLocalFailover,
            ..
        }
    )
}

pub(crate) fn should_fallback_to_control_sync(
    plan_kind: &str,
    result: &ExecutionResult,
    body_json: Option<&serde_json::Value>,
    has_body_bytes: bool,
    explicit_finalize: bool,
    mapped_error_finalize: bool,
) -> bool {
    if !matches!(
        plan_kind,
        "openai_chat_sync"
            | "openai_responses_sync"
            | "openai_responses_compact_sync"
            | "claude_chat_sync"
            | "gemini_chat_sync"
            | "claude_cli_sync"
            | "gemini_cli_sync"
    ) {
        return false;
    }

    if explicit_finalize {
        return result.status_code < 400 && body_json.is_none() && !has_body_bytes;
    }

    if mapped_error_finalize {
        return false;
    }

    if result.status_code >= 400 {
        return true;
    }

    let Some(body_json) = body_json else {
        return true;
    };

    body_json.get("error").is_some()
}

pub(crate) fn should_finalize_sync_response(report_kind: Option<&str>) -> bool {
    report_kind.is_some_and(|kind| kind.ends_with("_finalize"))
}

pub(crate) fn resolve_core_sync_error_finalize_report_kind(
    plan_kind: &str,
    result: &ExecutionResult,
    body_json: Option<&serde_json::Value>,
) -> Option<String> {
    let has_embedded_error = body_json.is_some_and(|value| value.get("error").is_some());
    if result.status_code < 400 && !has_embedded_error {
        return None;
    }

    let report_kind = match plan_kind {
        "openai_chat_sync" => "openai_chat_sync_finalize",
        "openai_responses_sync" => "openai_responses_sync_finalize",
        "openai_responses_compact_sync" => "openai_responses_compact_sync_finalize",
        "claude_chat_sync" => "claude_chat_sync_finalize",
        "gemini_chat_sync" => "gemini_chat_sync_finalize",
        "gemini_interactions_sync" => "gemini_interactions_sync_finalize",
        "claude_cli_sync" => "claude_cli_sync_finalize",
        "gemini_cli_sync" => "gemini_cli_sync_finalize",
        _ => return None,
    };

    Some(report_kind.to_string())
}

pub(crate) async fn should_retry_next_local_candidate_stream(
    state: &AppState,
    plan: &ExecutionPlan,
    _plan_kind: &str,
    report_context: Option<&serde_json::Value>,
    status_code: u16,
    response_text: Option<&str>,
) -> bool {
    matches!(
        resolve_local_candidate_failover_analysis_stream(
            state,
            plan,
            report_context,
            status_code,
            response_text,
        )
        .await,
        LocalFailoverAnalysis {
            decision: LocalFailoverDecision::RetryNextCandidate,
            ..
        }
    )
}

pub(crate) async fn should_stop_local_candidate_failover_stream(
    state: &AppState,
    plan: &ExecutionPlan,
    _plan_kind: &str,
    report_context: Option<&serde_json::Value>,
    status_code: u16,
    response_text: Option<&str>,
) -> bool {
    matches!(
        resolve_local_candidate_failover_analysis_stream(
            state,
            plan,
            report_context,
            status_code,
            response_text,
        )
        .await,
        LocalFailoverAnalysis {
            decision: LocalFailoverDecision::StopLocalFailover,
            ..
        }
    )
}

pub(crate) async fn resolve_local_candidate_failover_analysis_stream(
    state: &AppState,
    plan: &ExecutionPlan,
    report_context: Option<&serde_json::Value>,
    status_code: u16,
    response_text: Option<&str>,
) -> LocalFailoverAnalysis {
    if openai_image_success_disables_local_success_failover(plan, status_code) {
        return LocalFailoverAnalysis::use_default();
    }

    resolve_local_failover_analysis_for_attempt(
        state,
        plan,
        report_context,
        status_code,
        response_text,
    )
    .await
}

pub(crate) async fn resolve_local_candidate_failover_decision_stream(
    state: &AppState,
    plan: &ExecutionPlan,
    report_context: Option<&serde_json::Value>,
    status_code: u16,
    response_text: Option<&str>,
) -> LocalFailoverDecision {
    resolve_local_candidate_failover_analysis_stream(
        state,
        plan,
        report_context,
        status_code,
        response_text,
    )
    .await
    .decision
}

pub(crate) fn local_failover_response_text(
    body_json: Option<&serde_json::Value>,
    body_bytes: &[u8],
    fallback_text: Option<&str>,
) -> Option<String> {
    if let Some(body_json) = body_json {
        return serde_json::to_string(body_json).ok();
    }
    if !body_bytes.is_empty() {
        return Some(String::from_utf8_lossy(body_bytes).into_owned());
    }
    fallback_text
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

pub(crate) fn should_fallback_to_control_stream(
    plan_kind: &str,
    status_code: u16,
    mapped_error_finalize: bool,
) -> bool {
    if mapped_error_finalize {
        return false;
    }

    matches!(
        plan_kind,
        "openai_chat_stream"
            | "claude_chat_stream"
            | "gemini_chat_stream"
            | "openai_responses_stream"
            | "openai_responses_compact_stream"
            | "claude_cli_stream"
            | "gemini_cli_stream"
    ) && status_code >= 400
}

pub(crate) fn resolve_core_stream_error_finalize_report_kind(
    plan_kind: &str,
    status_code: u16,
) -> Option<String> {
    if status_code < 400 {
        return None;
    }

    let report_kind = match plan_kind {
        "openai_chat_stream" => "openai_chat_sync_finalize",
        "claude_chat_stream" => "claude_chat_sync_finalize",
        "gemini_chat_stream" => "gemini_chat_sync_finalize",
        "gemini_interactions_stream" => "gemini_interactions_sync_finalize",
        "openai_responses_stream" => "openai_responses_sync_finalize",
        "openai_responses_compact_stream" => "openai_responses_compact_sync_finalize",
        "claude_cli_stream" => "claude_cli_sync_finalize",
        "gemini_cli_stream" => "gemini_cli_sync_finalize",
        _ => return None,
    };

    Some(report_kind.to_string())
}

pub(crate) fn resolve_core_stream_direct_finalize_report_kind(plan_kind: &str) -> Option<String> {
    let report_kind = match plan_kind {
        "openai_chat_stream" => "openai_chat_sync_finalize",
        "openai_image_stream" => "openai_image_sync_finalize",
        "claude_chat_stream" => "claude_chat_sync_finalize",
        "gemini_chat_stream" => "gemini_chat_sync_finalize",
        "gemini_interactions_stream" => "gemini_interactions_sync_finalize",
        "openai_responses_stream" => "openai_responses_sync_finalize",
        "openai_responses_compact_stream" => "openai_responses_compact_sync_finalize",
        "claude_cli_stream" => "claude_cli_sync_finalize",
        "gemini_cli_stream" => "gemini_cli_sync_finalize",
        _ => return None,
    };

    Some(report_kind.to_string())
}
