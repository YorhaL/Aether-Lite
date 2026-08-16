use std::collections::{BTreeMap, VecDeque};
use std::future::Future;
use std::io::Error as IoError;
use std::pin::Pin;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    Arc,
};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use aether_ai_serving::{AiAttemptExecutionOutcome, AiAttemptRetryScope};
use aether_contracts::{
    ExecutionPlan, ExecutionResponseObservation, ExecutionStreamTerminalSummary,
    ExecutionTelemetry, StandardizedUsage, StreamFrame, StreamFramePayload,
};
use aether_data_contracts::repository::candidates::{
    RequestCandidateStatus, UpsertRequestCandidateRecord,
};
use aether_data_contracts::repository::usage::UsageBodyCaptureState;
use aether_scheduler_core::{
    parse_request_candidate_report_context, SchedulerRequestCandidateStatusUpdate,
};
use aether_usage_runtime::{
    build_lifecycle_usage_seed, build_stream_terminal_usage_payload_seed,
    build_sync_terminal_usage_payload_seed, build_terminal_usage_context_seed, LifecycleUsageSeed,
    SyncTerminalUsagePayloadSeed, TerminalUsageContextSeed, UsageRequestRecordLevel,
    DEFAULT_USAGE_RESPONSE_BODY_CAPTURE_LIMIT_BYTES,
};
use axum::body::{Body, Bytes};
use axum::http::Response;
use base64::Engine as _;
use futures_util::stream::{self as futures_stream, BoxStream};
use futures_util::{Stream, StreamExt, TryStreamExt};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tokio::io::{AsyncRead, ReadBuf};
use tokio_util::codec::{FramedRead, LinesCodec};
use tracing::{debug, info, warn};

use super::commit_policy::{
    anthropic_error_status_code, find_sse_record_boundary, StreamCommitGate, StreamCommitPolicy,
    StreamPrecommitObservation,
};
use super::error::{
    build_synthetic_non_success_stream_error_body, collect_error_body, decode_stream_error_body,
    inspect_prefetched_stream_body, read_next_frame,
    should_synthesize_non_success_stream_error_body,
    stream_client_error_status_code_for_upstream_status, synthetic_error_response_headers,
    StreamPrefetchInspection,
};
#[path = "execution_failures.rs"]
mod execution_failures;
use self::execution_failures::{
    build_stream_failure_from_execution_error, build_stream_failure_from_provider_error_body,
    build_stream_failure_report, build_stream_transport_failure_report,
    handle_prefetch_stream_failure, submit_midstream_stream_failure, StreamFailureReport,
};
use crate::ai_serving::api::StreamingStandardTerminalObserver;
use crate::ai_serving::is_openai_responses_family_format;
use crate::api::response::{
    attach_control_metadata_headers, build_client_response, build_client_response_from_parts,
};
use crate::clock::current_unix_ms as current_request_candidate_unix_ms;
use crate::constants::{CONTROL_CANDIDATE_ID_HEADER, CONTROL_REQUEST_ID_HEADER};
use crate::control::GatewayControlDecision;
use crate::execution_runtime::build_direct_execution_frame_stream;
#[cfg(test)]
use crate::execution_runtime::remote_test_support::post_stream_plan_to_remote_execution_runtime;
use crate::execution_runtime::submission::{
    resolve_core_error_background_report_kind, resolve_local_sync_error_status_code,
    strip_utf8_bom_and_ws, submit_local_core_error_or_sync_finalize,
};
use crate::execution_runtime::transport::{
    format_hyper_error_chain, format_upstream_request_error, format_wreq_upstream_request_error,
    stream_first_byte_timeout_message, DirectSyncExecutionRuntime, DirectUpstreamResponse,
    DirectUpstreamStreamExecution, ExecutionRuntimeTransportError,
};
use crate::execution_runtime::{
    ai_attempt_retry_scope_from_failure_disposition, apply_endpoint_response_header_rules,
    attach_provider_response_headers_to_report_context, local_failover_response_text,
    resolve_core_stream_direct_finalize_report_kind,
    resolve_core_stream_error_finalize_report_kind,
    resolve_local_candidate_failover_analysis_stream, should_fallback_to_control_stream,
    should_retry_next_local_candidate_stream, LocalFailoverDecision,
};
use crate::execution_runtime::{
    MAX_ERROR_BODY_BYTES, MAX_STREAM_PREFETCH_BYTES, MAX_STREAM_PREFETCH_FRAMES,
};
use crate::orchestration::{
    apply_local_execution_effect, build_local_error_flow_metadata, classify_failure_disposition,
    cyber_continue_failover_enabled, trace_upstream_response_body, with_error_flow_report_context,
    with_upstream_response_report_context, LocalAdaptiveRateLimitEffect,
    LocalAdaptiveSuccessEffect, LocalAttemptFailureEffect, LocalExecutionEffect,
    LocalExecutionEffectContext, LocalFailoverAnalysis, LocalHealthFailureEffect,
    LocalHealthSuccessEffect,
};
use crate::request_candidate_runtime::{
    ensure_execution_request_candidate_slot, persist_local_request_candidate_status_record,
    record_local_request_candidate_status, record_local_request_candidate_status_snapshot,
    snapshot_local_request_candidate_status, try_enqueue_local_request_candidate_status_snapshot,
    LocalRequestCandidateStatusSnapshot,
};
use crate::request_diagnostics::{
    attach_current_request_diagnostics_to_report_context,
    attach_request_diagnostics_and_candidate_start_timing_to_report_context,
    current_request_diagnostics, RequestDiagnostics,
};
use crate::stage_metrics::{
    attach_stage_trace_to_report_context, observe_gateway_stage_ms, observe_gateway_stage_trace_ms,
    RequestStageTrace,
};
use crate::usage::submit_stream_report;
use crate::usage::{GatewayStreamReportRequest, GatewaySyncReportRequest};
use crate::{
    AppState, GatewayError, GEMINI_FILES_DOWNLOAD_PLAN_KIND, OPENAI_VIDEO_CONTENT_PLAN_KIND,
};
use aether_gateway_frontdoor::short_request_id;

const OPENAI_IMAGE_STREAM_PLAN_KIND: &str = "openai_image_stream";
const SSE_CONTROL_FILTER_MAX_BUFFER_BYTES: usize = 1024 * 1024;
const SSE_TERMINAL_DETECTOR_MAX_LINE_BYTES: usize = 1024 * 1024;
const SSE_TERMINAL_DETECTOR_MAX_RECORD_BYTES: usize = SSE_TERMINAL_DETECTOR_MAX_LINE_BYTES;
const BASIC_STREAM_BODY_ANALYSIS_LIMIT_BYTES: usize = 5 * 1024 * 1024;
const STREAM_IDLE_LOG_INTERVAL: Duration = Duration::from_secs(60);
const STREAM_IDLE_LOG_INTERVAL_MS: u64 = 60_000;
const REWRITTEN_STREAM_PREFETCH_TIMEOUT: Duration = Duration::from_millis(750);
const ANTHROPIC_POST_STOP_DRAIN_MAX_WAIT: Duration = Duration::from_millis(250);
const ANTHROPIC_POST_STOP_DRAIN_MAX_FRAMES: usize = 8;
const ANTHROPIC_POST_STOP_DRAIN_MAX_BYTES: usize = 64 * 1024;
const POST_STOP_FRAME_READ_BUDGET_INACTIVE: usize = usize::MAX;
const POST_STOP_MAX_EMPTY_CHUNKS_PER_POLL: usize = 32;

#[derive(Debug)]
enum InProcessStreamExecutionError {
    Transport(ExecutionRuntimeTransportError),
    Gateway(GatewayError),
}

impl From<ExecutionRuntimeTransportError> for InProcessStreamExecutionError {
    fn from(error: ExecutionRuntimeTransportError) -> Self {
        Self::Transport(error)
    }
}

impl From<GatewayError> for InProcessStreamExecutionError {
    fn from(error: GatewayError) -> Self {
        Self::Gateway(error)
    }
}

fn report_context_with_stage_trace(
    report_context: Option<Value>,
    mut stage_trace: RequestStageTrace,
    stream_started_at: Instant,
    terminal_telemetry: Option<&ExecutionTelemetry>,
) -> Option<Value> {
    stage_trace.observe("stream_total", stream_elapsed_ms_since(stream_started_at));
    let fallback_elapsed_ms = terminal_telemetry.and_then(|telemetry| telemetry.ttfb_ms);
    attach_stage_trace_to_report_context(
        report_context,
        stage_trace.into_metadata_value(fallback_elapsed_ms),
    )
}

fn report_context_with_request_diagnostics(
    report_context: Option<Value>,
    diagnostics: Option<&Arc<RequestDiagnostics>>,
    candidate_started_at: Instant,
    terminal_telemetry: Option<&ExecutionTelemetry>,
) -> Option<Value> {
    attach_request_diagnostics_and_candidate_start_timing_to_report_context(
        report_context,
        diagnostics,
        Some(candidate_started_at),
        terminal_telemetry.and_then(|telemetry| telemetry.ttfb_ms),
    )
}

fn request_accepted_elapsed_ms(diagnostics: Option<&Arc<RequestDiagnostics>>) -> Option<u64> {
    diagnostics.and_then(|diagnostics| {
        diagnostics
            .request_accepted_at()
            .map(|accepted_at| accepted_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64)
    })
}

fn observe_request_accepted_stage_trace_ms(
    trace: &mut RequestStageTrace,
    diagnostics: Option<&Arc<RequestDiagnostics>>,
    stage: &'static str,
) {
    if let Some(elapsed_ms) = request_accepted_elapsed_ms(diagnostics) {
        observe_gateway_stage_trace_ms(trace, stage, elapsed_ms);
    }
}

fn stream_body_buffer_limit_for_record_level(record_level: UsageRequestRecordLevel) -> usize {
    match record_level {
        UsageRequestRecordLevel::Basic => BASIC_STREAM_BODY_ANALYSIS_LIMIT_BYTES,
        UsageRequestRecordLevel::Full => DEFAULT_USAGE_RESPONSE_BODY_CAPTURE_LIMIT_BYTES,
    }
}

async fn resolve_stream_body_buffer_limit(state: &AppState) -> usize {
    if !state.usage_runtime.is_enabled() {
        return BASIC_STREAM_BODY_ANALYSIS_LIMIT_BYTES;
    }

    match state
        .usage_runtime
        .body_capture_policy_for(state.usage_lifecycle_data_state().as_ref())
        .await
    {
        Ok(policy) => stream_body_buffer_limit_for_record_level(policy.record_level),
        Err(error) => {
            warn!(
                event_name = "stream_body_capture_policy_read_failed",
                log_type = "ops",
                error = %error,
                fallback = "full",
                "gateway could not resolve stream body capture policy"
            );
            DEFAULT_USAGE_RESPONSE_BODY_CAPTURE_LIMIT_BYTES
        }
    }
}

fn build_sync_terminal_usage_seeds(
    plan: &ExecutionPlan,
    report_context: Option<&serde_json::Value>,
    payload: &GatewaySyncReportRequest,
) -> (TerminalUsageContextSeed, SyncTerminalUsagePayloadSeed) {
    let report_context_with_diagnostics =
        attach_current_request_diagnostics_to_report_context(report_context);
    let context_seed = build_terminal_usage_context_seed(
        plan,
        report_context_with_diagnostics.as_ref().or(report_context),
    );
    let payload_seed = build_sync_terminal_usage_payload_seed(payload);
    (context_seed, payload_seed)
}

async fn record_sync_terminal_usage_with_handoff(
    state: &AppState,
    plan: &ExecutionPlan,
    report_context: Option<&serde_json::Value>,
    payload: &GatewaySyncReportRequest,
) {
    record_sync_terminal_usage_with_handoff_after_spawn(
        state,
        plan,
        report_context,
        payload,
        std::future::ready(()),
    )
    .await;
}

async fn record_sync_terminal_usage_with_handoff_after_spawn<F>(
    state: &AppState,
    plan: &ExecutionPlan,
    report_context: Option<&serde_json::Value>,
    payload: &GatewaySyncReportRequest,
    before_dispatch: F,
) where
    F: Future<Output = ()> + Send + 'static,
{
    crate::execution_runtime::mark_stream_candidate_watchdog_terminal_started();
    // Capture request task-local diagnostics before handing the work to a spawned task. Tokio
    // task-local values do not propagate across spawn boundaries.
    let (context_seed, payload_seed) =
        build_sync_terminal_usage_seeds(plan, report_context, payload);
    let state = state.clone();
    let task = tokio::spawn(async move {
        before_dispatch.await;
        state
            .usage_runtime
            .record_sync_terminal(
                state.usage_lifecycle_data_state().as_ref(),
                context_seed,
                payload_seed,
            )
            .await;
    });
    if let Err(err) = task.await {
        warn!(
            event_name = "sync_terminal_usage_handoff_failed",
            log_type = "ops",
            error = %err,
            "gateway sync terminal usage handoff task failed"
        );
    }
}

fn build_stream_sync_payload(
    trace_id: &str,
    report_kind: String,
    report_context: Option<Value>,
    status_code: u16,
    headers: BTreeMap<String, String>,
    body_json: Option<Value>,
    body_base64: Option<String>,
    telemetry: Option<ExecutionTelemetry>,
) -> GatewaySyncReportRequest {
    GatewaySyncReportRequest {
        trace_id: trace_id.to_string(),
        report_kind,
        report_context,
        status_code,
        headers,
        body_json,
        client_body_json: None,
        body_base64,
        telemetry,
    }
}

#[allow(clippy::too_many_arguments)]
fn build_stream_error_sync_payload(
    trace_id: &str,
    report_kind: String,
    report_context: Option<Value>,
    upstream_status_code: u16,
    provider_headers: BTreeMap<String, String>,
    provider_body_json: Option<Value>,
    provider_body_base64: Option<String>,
    client_headers: BTreeMap<String, String>,
    client_body_json: Option<Value>,
    telemetry: Option<ExecutionTelemetry>,
) -> GatewaySyncReportRequest {
    let client_status_code =
        stream_client_error_status_code_for_upstream_status(upstream_status_code);
    let mut report_context = report_context;
    if client_status_code != upstream_status_code || client_headers != provider_headers {
        let mut object = match report_context {
            Some(Value::Object(object)) => object,
            Some(other) => serde_json::Map::from_iter([("seed".to_string(), other)]),
            None => serde_json::Map::new(),
        };
        object.insert(
            "client_response_status_code".to_string(),
            Value::from(client_status_code),
        );
        object.insert(
            "client_response_headers".to_string(),
            serde_json::to_value(client_headers).unwrap_or(Value::Null),
        );
        report_context = Some(Value::Object(object));
    }

    GatewaySyncReportRequest {
        trace_id: trace_id.to_string(),
        report_kind,
        report_context,
        status_code: upstream_status_code,
        headers: provider_headers,
        body_json: provider_body_json,
        client_body_json,
        body_base64: provider_body_base64,
        telemetry,
    }
}

async fn record_stream_terminal_usage(
    state: &AppState,
    plan: &ExecutionPlan,
    report_context: Option<&serde_json::Value>,
    payload: &GatewayStreamReportRequest,
    cancelled: bool,
) {
    crate::execution_runtime::mark_stream_candidate_watchdog_terminal_started();
    let context_seed = build_terminal_usage_context_seed(plan, report_context);
    let payload_seed = build_stream_terminal_usage_payload_seed(payload);
    state
        .usage_runtime
        .record_stream_terminal(
            state.usage_lifecycle_data_state().as_ref(),
            context_seed,
            payload_seed,
            cancelled,
        )
        .await;
}

async fn record_stream_admission_timeout_candidate_failure(
    state: &AppState,
    plan: &ExecutionPlan,
    report_context: Option<&Value>,
    candidate_started_unix_ms: u64,
    error: &GatewayError,
) {
    let status_code = 429;
    let error_type = "gateway_admission_timeout";
    let error_message = match error {
        GatewayError::AdmissionTimeout {
            gate,
            queue_budget_ms,
            ..
        } => format!("gateway admission gate {gate} timed out after {queue_budget_ms}ms"),
        other => format!("{other:?}"),
    };
    let terminal_unix_ms = current_request_candidate_unix_ms();
    let latency_ms = terminal_unix_ms.saturating_sub(candidate_started_unix_ms);
    record_local_request_candidate_status(
        state,
        plan,
        report_context,
        SchedulerRequestCandidateStatusUpdate {
            status: RequestCandidateStatus::Failed,
            status_code: Some(status_code),
            error_type: Some(error_type.to_string()),
            error_message: Some(error_message.clone()),
            latency_ms: Some(latency_ms),
            started_at_unix_ms: Some(candidate_started_unix_ms),
            finished_at_unix_ms: Some(terminal_unix_ms),
        },
    )
    .await;
}

fn build_stream_body_capture(
    body: &[u8],
    truncated: bool,
) -> (Option<String>, Option<UsageBodyCaptureState>) {
    let body_base64 =
        (!body.is_empty()).then(|| base64::engine::general_purpose::STANDARD.encode(body));
    let body_state = Some(if truncated {
        UsageBodyCaptureState::Truncated
    } else if body.is_empty() {
        UsageBodyCaptureState::None
    } else {
        UsageBodyCaptureState::Inline
    });
    (body_base64, body_state)
}

fn wrap_non_json_binary_stream_error_for_client(
    plan_kind: &str,
    headers: &BTreeMap<String, String>,
    error_body: &[u8],
) -> Result<Option<Value>, GatewayError> {
    let content_type = headers
        .get("content-type")
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_default();
    if content_type.starts_with("application/json") {
        return Ok(None);
    }

    let body = match plan_kind {
        GEMINI_FILES_DOWNLOAD_PLAN_KIND => json!({
            "error": String::from_utf8_lossy(error_body).to_string(),
        }),
        OPENAI_VIDEO_CONTENT_PLAN_KIND => json!({
            "error": {
                "type": "upstream_error",
                "message": "Video not available",
            }
        }),
        _ => return Ok(None),
    };
    Ok(Some(body))
}

fn with_stream_error_trace_context(
    report_context: Option<&Value>,
    status_code: u16,
    headers: &BTreeMap<String, String>,
    body_json: Option<&Value>,
    body_bytes: &[u8],
    response_text: Option<&str>,
    local_failover_analysis: crate::orchestration::LocalFailoverAnalysis,
) -> Option<Value> {
    let body = trace_upstream_response_body(body_json, body_bytes);
    let upstream_context = with_upstream_response_report_context(
        report_context,
        status_code,
        Some(headers),
        body.as_ref(),
        None,
        None,
    );
    with_error_flow_report_context(
        upstream_context.as_ref().or(report_context),
        build_local_error_flow_metadata(status_code, response_text, local_failover_analysis),
    )
}

#[allow(clippy::too_many_arguments)] // stream report payload assembly mirrors runtime state
fn build_stream_usage_payload(
    trace_id: String,
    report_kind: String,
    report_context: Option<Value>,
    status_code: u16,
    headers: BTreeMap<String, String>,
    provider_body: &[u8],
    provider_body_truncated: bool,
    client_body: &[u8],
    client_body_truncated: bool,
    terminal_summary: Option<ExecutionStreamTerminalSummary>,
    telemetry: Option<ExecutionTelemetry>,
) -> GatewayStreamReportRequest {
    let (provider_body_base64, provider_body_state) =
        build_stream_body_capture(provider_body, provider_body_truncated);
    let (client_body_base64, client_body_state) =
        build_stream_body_capture(client_body, client_body_truncated);
    GatewayStreamReportRequest {
        trace_id,
        report_kind,
        report_context,
        status_code,
        headers,
        provider_body_base64,
        provider_body_state,
        client_body_base64,
        client_body_state,
        terminal_summary,
        telemetry,
    }
}

fn append_stream_capture_bytes(
    buffer: &mut Vec<u8>,
    chunk: &[u8],
    max_bytes: usize,
    truncated: &mut bool,
) {
    if chunk.is_empty() || max_bytes == 0 {
        return;
    }
    if buffer.len() >= max_bytes {
        *truncated = true;
        return;
    }
    let remaining = max_bytes - buffer.len();
    let keep_len = remaining.min(chunk.len());
    buffer.extend_from_slice(&chunk[..keep_len]);
    if keep_len < chunk.len() {
        *truncated = true;
    }
}

fn observe_stream_usage_bytes(
    observer: &mut StreamingStandardTerminalObserver,
    report_context: &Value,
    buffered: &mut Vec<u8>,
    chunk: &[u8],
) {
    if chunk.is_empty()
        || observer
            .latest_summary()
            .and_then(|summary| summary.parser_error.as_deref())
            .is_some()
    {
        return;
    }

    let mut remaining = chunk;
    while !remaining.is_empty() {
        let line_part_len = remaining
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(remaining.len(), |index| index + 1);
        if buffered.len().saturating_add(line_part_len) > SSE_TERMINAL_DETECTOR_MAX_LINE_BYTES {
            observer.disable_with_error(format!(
                "stream usage event exceeded {SSE_TERMINAL_DETECTOR_MAX_LINE_BYTES} bytes"
            ));
            buffered.clear();
            return;
        }
        buffered.extend_from_slice(&remaining[..line_part_len]);
        remaining = &remaining[line_part_len..];
        if buffered.last() == Some(&b'\n') {
            let line = std::mem::take(buffered);
            observer.push_line(report_context, line);
        }
    }
}

fn finalize_stream_usage_observer(
    observer: &mut Option<StreamingStandardTerminalObserver>,
    report_context: Option<&Value>,
    buffered: &mut Vec<u8>,
) -> Option<ExecutionStreamTerminalSummary> {
    let (Some(observer), Some(report_context)) = (observer.as_mut(), report_context) else {
        return None;
    };

    if !buffered.is_empty() {
        let line = std::mem::take(buffered);
        observer.push_line(report_context, line);
    }
    observer.finish(report_context)
}

fn merge_stream_terminal_summary(
    mut current: Option<ExecutionStreamTerminalSummary>,
    observed: Option<ExecutionStreamTerminalSummary>,
) -> Option<ExecutionStreamTerminalSummary> {
    let Some(observed) = observed else {
        return current;
    };

    let Some(current_summary) = current.as_mut() else {
        return Some(observed);
    };

    if should_replace_stream_usage(
        current_summary.standardized_usage.as_ref(),
        observed.standardized_usage.as_ref(),
    ) {
        current_summary.standardized_usage = observed.standardized_usage;
    }
    if current_summary.finish_reason.is_none() {
        current_summary.finish_reason = observed.finish_reason;
    }
    if current_summary.response_id.is_none() {
        current_summary.response_id = observed.response_id;
    }
    if current_summary.model.is_none() {
        current_summary.model = observed.model;
    }
    if observed.provider_actual_service_tier.is_some() {
        current_summary.provider_actual_service_tier = observed.provider_actual_service_tier;
    }
    current_summary.observed_finish |= observed.observed_finish;
    current_summary.unknown_event_count = current_summary
        .unknown_event_count
        .saturating_add(observed.unknown_event_count);
    if current_summary.parser_error.is_none() {
        current_summary.parser_error = observed.parser_error;
    }

    current
}

fn should_replace_stream_usage(
    current: Option<&aether_contracts::StandardizedUsage>,
    observed: Option<&aether_contracts::StandardizedUsage>,
) -> bool {
    let Some(observed) = observed else {
        return false;
    };
    let Some(current) = current else {
        return true;
    };

    observed.is_more_complete_than(current)
}

fn stream_terminal_summary_missing_observed_finish(
    summary: Option<&ExecutionStreamTerminalSummary>,
) -> bool {
    summary.is_some_and(|summary| {
        !summary.observed_finish
            && !summary
                .standardized_usage
                .as_ref()
                .is_some_and(StandardizedUsage::has_token_signal)
    })
}

fn stream_report_context_format_field<'a>(
    report_context: Option<&'a Value>,
    field: &str,
) -> Option<&'a str> {
    report_context
        .and_then(Value::as_object)
        .and_then(|object| object.get(field))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn stream_requires_observed_terminal_event(
    provider_api_format: &str,
    report_context: Option<&Value>,
) -> bool {
    is_openai_responses_family_format(provider_api_format)
        || [
            "provider_stream_event_api_format",
            "provider_stream_api_format",
            "provider_api_format",
        ]
        .into_iter()
        .filter_map(|field| stream_report_context_format_field(report_context, field))
        .any(is_openai_responses_family_format)
}

fn stream_terminal_summary_missing_observed_finish_with_requirement(
    summary: Option<&ExecutionStreamTerminalSummary>,
    requires_observed_terminal_event: bool,
) -> bool {
    if !requires_observed_terminal_event {
        return stream_terminal_summary_missing_observed_finish(summary);
    }

    summary.is_some_and(|summary| !summary.observed_finish)
}

fn ensure_stream_terminal_summary_for_missing_observed_finish(
    summary: &mut Option<ExecutionStreamTerminalSummary>,
    requires_observed_terminal_event: bool,
) {
    if !requires_observed_terminal_event {
        return;
    }

    let summary = summary.get_or_insert_with(ExecutionStreamTerminalSummary::default);
    if !summary.observed_finish && summary.parser_error.is_none() {
        summary.parser_error =
            Some("execution runtime stream ended before provider terminal event".to_string());
    }
}

fn stream_terminal_summary_represents_failure_with_requirement(
    summary: Option<&ExecutionStreamTerminalSummary>,
    requires_observed_terminal_event: bool,
) -> bool {
    summary.is_some_and(|summary| {
        summary.parser_error.is_some()
            || stream_terminal_summary_missing_observed_finish_with_requirement(
                Some(summary),
                requires_observed_terminal_event,
            )
    })
}

async fn execute_in_process_stream(
    state: &AppState,
    plan: &ExecutionPlan,
    trace_id: &str,
) -> Result<DirectUpstreamStreamExecution, InProcessStreamExecutionError> {
    let upstream_target_permit = state
        .upstream_target_admission
        .acquire(plan, trace_id)
        .await?;
    match DirectSyncExecutionRuntime::new().execute_stream(plan).await {
        Ok(mut execution) => {
            execution.upstream_target_permit = upstream_target_permit;
            Ok(execution)
        }
        Err(error) => Err(error.into()),
    }
}

async fn execute_in_process_stream_with_report_context(
    state: &AppState,
    plan: &ExecutionPlan,
    trace_id: &str,
    report_context: Option<&Value>,
) -> Result<DirectUpstreamStreamExecution, InProcessStreamExecutionError> {
    let mut execution = execute_in_process_stream(state, plan, trace_id).await?;
    apply_stream_summary_report_context(&mut execution, report_context);
    Ok(execution)
}

fn should_use_direct_sse_passthrough(
    plan: &ExecutionPlan,
    plan_kind: &str,
    report_context: Option<&Value>,
    execution: &DirectUpstreamStreamExecution,
) -> bool {
    if !(200..300).contains(&execution.status_code) {
        return false;
    }
    if !plan
        .provider_api_format
        .eq_ignore_ascii_case(plan.client_api_format.as_str())
    {
        return false;
    }
    if client_format_allows_proxy_generated_sse_control_blocks(plan) {
        return false;
    }
    let _ = (plan_kind, report_context);
    true
}

type DirectUpstreamByteStream = BoxStream<'static, Result<Bytes, String>>;

fn direct_upstream_response_byte_stream(
    prefetched_body: VecDeque<Result<Bytes, String>>,
    response: DirectUpstreamResponse,
) -> DirectUpstreamByteStream {
    let response_stream = match response {
        DirectUpstreamResponse::Reqwest(response) => response
            .bytes_stream()
            .map(|item| item.map_err(|err| format_upstream_request_error(&err)))
            .boxed(),
        DirectUpstreamResponse::HyperH2c(response) => response
            .into_body()
            .into_data_stream()
            .map(|item| item.map_err(|err| format_hyper_error_chain(&err)))
            .boxed(),
        DirectUpstreamResponse::BrowserWreq(response) => response
            .bytes_stream()
            .map(|item| item.map_err(|err| format_wreq_upstream_request_error(&err)))
            .boxed(),
    };
    futures_stream::iter(prefetched_body)
        .chain(response_stream)
        .boxed()
}

async fn await_direct_passthrough_first_item<T, F>(
    future: F,
    started_at: Instant,
    timeout: Option<Duration>,
) -> Result<T, Duration>
where
    F: Future<Output = T>,
{
    let Some(timeout) = timeout else {
        return Ok(future.await);
    };
    let Some(remaining) = timeout.checked_sub(started_at.elapsed()) else {
        return Err(timeout);
    };
    if remaining.is_zero() {
        return Err(timeout);
    }
    tokio::time::timeout(remaining, future)
        .await
        .map_err(|_| timeout)
}

struct DirectPassthroughFinalizer {
    core: Option<DirectPassthroughFinalizerCore>,
}

struct DirectPassthroughFinalizerCore {
    state: AppState,
    plan: ExecutionPlan,
    trace_id: String,
    report_kind: Option<String>,
    report_context: Option<Value>,
    lifecycle_seed: LifecycleUsageSeed,
    direct_stream_finalize_kind: Option<String>,
    stream_started_at: Instant,
    stage_trace: RequestStageTrace,
    request_diagnostics: Option<Arc<RequestDiagnostics>>,
    request_id_for_log: String,
    candidate_id: Option<String>,
    request_candidate_status_snapshot: Option<LocalRequestCandidateStatusSnapshot>,
    deferred_request_candidate_status_record: Option<UpsertRequestCandidateRecord>,
    candidate_started_unix_secs: u64,
    status_code: u16,
    headers: BTreeMap<String, String>,
    stream_usage_report_context: Option<Value>,
    stream_usage_observer: Option<StreamingStandardTerminalObserver>,
    stream_usage_observer_buffered: Vec<u8>,
    max_stream_body_buffer_bytes: usize,
    provider_buffered_body: Vec<u8>,
    buffered_body: Vec<u8>,
    provider_body_truncated: bool,
    client_body_truncated: bool,
    client_stream_completion_tracker: ClientVisibleStreamCompletionTracker,
    requires_anthropic_message_stop: bool,
    client_visible_stream_completed: bool,
    usage_stream_telemetry: Option<ExecutionTelemetry>,
    telemetry: Option<ExecutionTelemetry>,
    provider_stream_bytes: u64,
    client_stream_bytes: u64,
    last_client_chunk_elapsed_ms: u64,
    pending_recorded: bool,
    stream_started_recorded: bool,
    terminal_failure: Option<StreamFailureReport>,
    _upstream_target_permit: Option<crate::upstream_admission::UpstreamTargetAdmissionPermit>,
}

impl DirectPassthroughFinalizer {
    fn new(core: DirectPassthroughFinalizerCore) -> Self {
        Self { core: Some(core) }
    }

    fn core(&self) -> &DirectPassthroughFinalizerCore {
        self.core
            .as_ref()
            .expect("direct passthrough finalizer core should exist")
    }

    fn core_mut(&mut self) -> &mut DirectPassthroughFinalizerCore {
        self.core
            .as_mut()
            .expect("direct passthrough finalizer core should exist")
    }

    fn stream_started_at(&self) -> Instant {
        self.core().stream_started_at
    }

    fn ttfb_observed(&self) -> bool {
        self.core()
            .usage_stream_telemetry
            .as_ref()
            .and_then(|telemetry| telemetry.ttfb_ms)
            .is_some()
    }

    fn terminal_failure(&self) -> Option<&StreamFailureReport> {
        self.core().terminal_failure.as_ref()
    }

    fn set_terminal_failure(&mut self, failure: StreamFailureReport) {
        self.core_mut().terminal_failure = Some(failure);
    }

    fn prepare_upstream_chunk(&mut self, mut chunk: Bytes) -> Option<Bytes> {
        let core = self.core_mut();
        if !core.requires_anthropic_message_stop {
            return Some(chunk);
        }
        if core.client_visible_stream_completed {
            return None;
        }
        if let Some(terminal_end) = core
            .client_stream_completion_tracker
            .observe_anthropic_message_stop_terminal_end(chunk.as_ref())
        {
            chunk.truncate(terminal_end);
            core.client_visible_stream_completed = true;
        }
        Some(chunk)
    }

    fn fail_if_anthropic_message_stop_missing(&mut self) {
        let core = self.core_mut();
        if core.requires_anthropic_message_stop
            && !core.client_visible_stream_completed
            && core.terminal_failure.is_none()
        {
            core.terminal_failure = Some(build_anthropic_premature_eof_failure(
                "upstream Anthropic stream ended before message_stop",
            ));
        }
    }

    fn completed_native_anthropic_stream(&self) -> bool {
        let core = self.core();
        core.requires_anthropic_message_stop
            && core.client_visible_stream_completed
            && core.terminal_failure.is_none()
    }

    fn log_terminal_error_event_encode_failed(&self, err: impl std::fmt::Debug) {
        let core = self.core();
        warn!(
            event_name = "direct_passthrough_terminal_error_event_encode_failed",
            log_type = "ops",
            trace_id = %core.trace_id,
            request_id = %core.request_id_for_log,
            candidate_id = ?core.candidate_id.as_deref(),
            error = ?err,
            "gateway direct passthrough failed to encode terminal SSE error event"
        );
    }

    fn observe_first_body_poll(&mut self) {
        let core = self.core_mut();
        observe_gateway_stage_trace_ms(
            &mut core.stage_trace,
            "stream_body_inline_first_poll",
            stream_elapsed_ms_since(core.stream_started_at),
        );
        let request_diagnostics = core.request_diagnostics.clone();
        observe_request_accepted_stage_trace_ms(
            &mut core.stage_trace,
            request_diagnostics.as_ref(),
            "frontdoor_to_stream_body_first_poll",
        );
    }

    fn observe_upstream_chunk(&mut self, chunk: &Bytes, observed_at: Instant) {
        let core = self.core_mut();
        if core.provider_stream_bytes == 0 {
            observe_gateway_stage_trace_ms(
                &mut core.stage_trace,
                "direct_passthrough_upstream_body_first",
                stream_elapsed_ms_at(core.stream_started_at, observed_at),
            );
        }
        let captured_first_stream_event = maybe_capture_first_stream_event_telemetry(
            core.stream_started_at,
            observed_at,
            core.telemetry.as_ref(),
            &mut core.usage_stream_telemetry,
        );
        if captured_first_stream_event && core.provider_stream_bytes == 0 {
            observe_gateway_stage_trace_ms(
                &mut core.stage_trace,
                "stream_first_data",
                stream_elapsed_ms_at(core.stream_started_at, observed_at),
            );
        }

        core.provider_stream_bytes = core
            .provider_stream_bytes
            .saturating_add(u64::try_from(chunk.len()).unwrap_or(u64::MAX));
        append_stream_capture_bytes(
            &mut core.provider_buffered_body,
            chunk.as_ref(),
            core.max_stream_body_buffer_bytes,
            &mut core.provider_body_truncated,
        );
        if let (Some(observer), Some(report_context)) = (
            core.stream_usage_observer.as_mut(),
            core.stream_usage_report_context.as_ref(),
        ) {
            observe_stream_usage_bytes(
                observer,
                report_context,
                &mut core.stream_usage_observer_buffered,
                chunk.as_ref(),
            );
        }
    }

    fn observe_client_chunk(&mut self, chunk: &Bytes) {
        if chunk.is_empty() {
            return;
        }
        let core = self.core_mut();
        append_stream_capture_bytes(
            &mut core.buffered_body,
            chunk.as_ref(),
            core.max_stream_body_buffer_bytes,
            &mut core.client_body_truncated,
        );
        if !core.requires_anthropic_message_stop {
            core.client_visible_stream_completed |= core
                .client_stream_completion_tracker
                .observe_chunk(chunk.as_ref());
        }
        core.client_stream_bytes = core
            .client_stream_bytes
            .saturating_add(u64::try_from(chunk.len()).unwrap_or(u64::MAX));
        core.last_client_chunk_elapsed_ms = core
            .stream_started_at
            .elapsed()
            .as_millis()
            .min(u128::from(u64::MAX)) as u64;
    }

    fn observe_first_client_yield(&mut self) {
        let core = self.core_mut();
        let elapsed_ms = stream_elapsed_ms_since(core.stream_started_at);
        observe_gateway_stage_trace_ms(
            &mut core.stage_trace,
            "direct_passthrough_first_client_send",
            elapsed_ms,
        );
        observe_gateway_stage_trace_ms(
            &mut core.stage_trace,
            "stream_first_client_yield",
            elapsed_ms,
        );
        let request_diagnostics = core.request_diagnostics.clone();
        observe_request_accepted_stage_trace_ms(
            &mut core.stage_trace,
            request_diagnostics.as_ref(),
            "frontdoor_to_stream_first_client_yield",
        );
    }

    fn release_upstream_target_permit_after_first_yield(&mut self) {
        let core = self.core_mut();
        if core._upstream_target_permit.take().is_some() {
            observe_gateway_stage_trace_ms(
                &mut core.stage_trace,
                "stream_upstream_target_permit_release",
                stream_elapsed_ms_since(core.stream_started_at),
            );
        }
    }

    fn record_client_visible_stream_started_if_needed(&mut self) {
        let Some(core) = self.core.as_mut() else {
            return;
        };
        core.record_client_visible_stream_started_if_needed();
    }

    async fn finalize(&mut self, downstream_dropped: bool) {
        let Some(core) = self.core.take() else {
            return;
        };
        // Move the owned terminal payload into a task before awaiting it. A
        // client disconnect or an execution timeout may cancel this body
        // future while terminal admission is backpressured; the handoff must
        // continue independently so the usage row cannot remain streaming.
        let task = tokio::spawn(async move {
            core.finalize(downstream_dropped).await;
        });
        if let Err(err) = task.await {
            warn!(
                event_name = "direct_passthrough_terminal_handoff_failed",
                log_type = "ops",
                error = %err,
                "gateway direct passthrough terminal handoff task failed"
            );
        }
    }
}

impl Drop for DirectPassthroughFinalizer {
    fn drop(&mut self) {
        let Some(core) = self.core.take() else {
            return;
        };
        observe_gateway_stage_ms("stream_finalizer_enqueue", 0);
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                core.finalize(true).await;
            });
        }
    }
}

fn enqueue_stream_candidate_status_update(
    state: &AppState,
    snapshot: LocalRequestCandidateStatusSnapshot,
    status_update: SchedulerRequestCandidateStatusUpdate,
) -> Option<UpsertRequestCandidateRecord> {
    let Err(record) =
        try_enqueue_local_request_candidate_status_snapshot(state, &snapshot, status_update)
    else {
        return None;
    };
    if state.request_candidate_queue.is_some() {
        return Some(record);
    }

    // Without an async queue, preserve the first-byte path's existing
    // fire-and-handoff behavior. Queue saturation uses the bounded deferred
    // record above and does not create one task per waiter.
    let state = state.clone();
    tokio::spawn(async move {
        persist_local_request_candidate_status_record(&state, record).await;
    });
    None
}

impl DirectPassthroughFinalizerCore {
    fn record_client_visible_stream_started_if_needed(&mut self) {
        if self.stream_started_recorded || self.client_stream_bytes == 0 {
            return;
        }
        self.stream_started_recorded = true;
        if !self.pending_recorded {
            self.pending_recorded = true;
            self.state.usage_runtime.record_pending(
                self.state.usage_lifecycle_data_state().as_ref(),
                self.lifecycle_seed.clone(),
            );
        }
        self.state.usage_runtime.record_stream_started(
            self.state.usage_lifecycle_data_state().as_ref(),
            &self.lifecycle_seed,
            self.status_code,
            self.usage_stream_telemetry.as_ref(),
        );
        if let Some(snapshot) = self.request_candidate_status_snapshot.take() {
            self.deferred_request_candidate_status_record = enqueue_stream_candidate_status_update(
                &self.state,
                snapshot,
                SchedulerRequestCandidateStatusUpdate {
                    status: RequestCandidateStatus::Streaming,
                    status_code: Some(self.status_code),
                    error_type: None,
                    error_message: None,
                    latency_ms: None,
                    started_at_unix_ms: Some(self.candidate_started_unix_secs),
                    finished_at_unix_ms: None,
                },
            );
        }
    }

    async fn finalize(mut self, mut downstream_dropped: bool) {
        self.record_client_visible_stream_started_if_needed();
        observe_gateway_stage_ms(
            "stream_total",
            stream_elapsed_ms_since(self.stream_started_at),
        );
        let stream_terminal_summary = finalize_stream_usage_observer(
            &mut self.stream_usage_observer,
            self.stream_usage_report_context.as_ref(),
            &mut self.stream_usage_observer_buffered,
        );

        let DirectPassthroughFinalizerCore {
            state,
            plan,
            trace_id,
            report_kind,
            report_context,
            lifecycle_seed: _,
            direct_stream_finalize_kind,
            stream_started_at,
            stage_trace,
            request_diagnostics,
            request_id_for_log,
            candidate_id,
            request_candidate_status_snapshot: _,
            deferred_request_candidate_status_record,
            candidate_started_unix_secs,
            status_code,
            headers,
            stream_usage_report_context,
            stream_usage_observer: _,
            stream_usage_observer_buffered: _,
            max_stream_body_buffer_bytes: _,
            provider_buffered_body,
            buffered_body,
            provider_body_truncated,
            client_body_truncated,
            client_stream_completion_tracker: _,
            requires_anthropic_message_stop: _,
            client_visible_stream_completed,
            usage_stream_telemetry,
            telemetry,
            provider_stream_bytes,
            client_stream_bytes: _,
            last_client_chunk_elapsed_ms: _,
            pending_recorded: _,
            stream_started_recorded: _,
            terminal_failure,
            _upstream_target_permit,
        } = self;

        // Queue backpressure must not keep scarce upstream/provider permits
        // occupied after the client-visible stream has already ended.
        drop(_upstream_target_permit);
        if let Some(record) = deferred_request_candidate_status_record {
            persist_local_request_candidate_status_record(&state, record).await;
        }

        if downstream_dropped && client_visible_stream_completed && terminal_failure.is_none() {
            debug!(
                event_name = "direct_passthrough_downstream_closed_after_done",
                log_type = "debug",
                trace_id = %trace_id,
                request_id = %request_id_for_log,
                candidate_id = ?candidate_id.as_deref(),
                "gateway treats direct passthrough downstream close after terminal SSE event as completed"
            );
            downstream_dropped = false;
        }

        if let Some(failure) = terminal_failure {
            let terminal_telemetry = Some(build_terminal_stream_telemetry(
                stream_started_at,
                telemetry.as_ref(),
                usage_stream_telemetry.as_ref(),
                provider_stream_bytes,
            ));
            let report_context_for_payload = report_context_with_stage_trace(
                report_context,
                stage_trace,
                stream_started_at,
                terminal_telemetry.as_ref(),
            );
            let report_context_for_payload = report_context_with_request_diagnostics(
                report_context_for_payload,
                request_diagnostics.as_ref(),
                stream_started_at,
                terminal_telemetry.as_ref(),
            );
            submit_midstream_stream_failure(
                &state,
                &trace_id,
                &plan,
                direct_stream_finalize_kind.as_deref(),
                report_context_for_payload,
                headers,
                terminal_telemetry,
                &provider_buffered_body,
                candidate_started_unix_secs,
                failure,
            )
            .await;
            return;
        }

        if downstream_dropped {
            let terminal_telemetry = Some(build_terminal_stream_telemetry(
                stream_started_at,
                telemetry.as_ref(),
                usage_stream_telemetry.as_ref(),
                provider_stream_bytes,
            ));
            let report_context_for_payload = report_context_with_stage_trace(
                report_context,
                stage_trace,
                stream_started_at,
                terminal_telemetry.as_ref(),
            );
            let report_context_for_payload = report_context_with_request_diagnostics(
                report_context_for_payload,
                request_diagnostics.as_ref(),
                stream_started_at,
                terminal_telemetry.as_ref(),
            );
            let usage_payload = build_stream_usage_payload(
                trace_id,
                report_kind.unwrap_or_default(),
                report_context_for_payload,
                499,
                headers,
                &provider_buffered_body,
                provider_body_truncated,
                &buffered_body,
                client_body_truncated,
                stream_terminal_summary,
                terminal_telemetry,
            );
            record_stream_terminal_usage(
                &state,
                &plan,
                usage_payload.report_context.as_ref(),
                &usage_payload,
                true,
            )
            .await;
            record_local_request_candidate_status(
                &state,
                &plan,
                usage_payload.report_context.as_ref(),
                SchedulerRequestCandidateStatusUpdate {
                    status: RequestCandidateStatus::Cancelled,
                    status_code: Some(499),
                    error_type: Some("downstream_disconnect".to_string()),
                    error_message: Some("client disconnected before stream completion".to_string()),
                    latency_ms: usage_payload
                        .telemetry
                        .as_ref()
                        .and_then(|value| value.elapsed_ms),
                    started_at_unix_ms: Some(candidate_started_unix_secs),
                    finished_at_unix_ms: Some(current_request_candidate_unix_ms()),
                },
            )
            .await;
            return;
        }

        let mut stream_terminal_summary = stream_terminal_summary;
        let requires_observed_terminal_event = stream_requires_observed_terminal_event(
            plan.provider_api_format.as_str(),
            stream_usage_report_context.as_ref(),
        );
        ensure_stream_terminal_summary_for_missing_observed_finish(
            &mut stream_terminal_summary,
            requires_observed_terminal_event,
        );
        let missing_observed_finish =
            stream_terminal_summary_missing_observed_finish_with_requirement(
                stream_terminal_summary.as_ref(),
                requires_observed_terminal_event,
            );
        let stream_failed = stream_terminal_summary_represents_failure_with_requirement(
            stream_terminal_summary.as_ref(),
            requires_observed_terminal_event,
        );
        let stream_terminal_error_message = stream_terminal_summary
            .as_ref()
            .and_then(|summary| summary.parser_error.clone())
            .or_else(|| {
                missing_observed_finish.then(|| {
                    "execution runtime stream ended before provider terminal event".to_string()
                })
            });
        let should_submit_report = report_kind.is_some();
        let terminal_telemetry = Some(build_terminal_stream_telemetry(
            stream_started_at,
            telemetry.as_ref(),
            usage_stream_telemetry.as_ref(),
            provider_stream_bytes,
        ));
        let report_context_for_payload = report_context_with_stage_trace(
            report_context,
            stage_trace,
            stream_started_at,
            terminal_telemetry.as_ref(),
        );
        let report_context_for_payload = report_context_with_request_diagnostics(
            report_context_for_payload,
            request_diagnostics.as_ref(),
            stream_started_at,
            terminal_telemetry.as_ref(),
        );
        let usage_payload = build_stream_usage_payload(
            trace_id.clone(),
            report_kind.unwrap_or_default(),
            report_context_for_payload,
            status_code,
            headers,
            &provider_buffered_body,
            provider_body_truncated,
            &buffered_body,
            client_body_truncated,
            stream_terminal_summary,
            terminal_telemetry,
        );
        if stream_failed {
            warn!(
                event_name = "direct_passthrough_stream_failed",
                log_type = "ops",
                trace_id = %trace_id,
                request_id = %request_id_for_log,
                candidate_id = ?candidate_id.as_deref(),
                status_code,
                error_message = stream_terminal_error_message.as_deref().unwrap_or_default(),
                "gateway direct passthrough stream ended with a failed terminal state"
            );
        } else {
            apply_local_execution_effect(
                &state,
                LocalExecutionEffectContext {
                    plan: &plan,
                    report_context: usage_payload.report_context.as_ref(),
                },
                LocalExecutionEffect::HealthSuccess(LocalHealthSuccessEffect),
            )
            .await;
            apply_local_execution_effect(
                &state,
                LocalExecutionEffectContext {
                    plan: &plan,
                    report_context: usage_payload.report_context.as_ref(),
                },
                LocalExecutionEffect::AdaptiveSuccess(LocalAdaptiveSuccessEffect),
            )
            .await;
        }
        record_stream_terminal_usage(
            &state,
            &plan,
            usage_payload.report_context.as_ref(),
            &usage_payload,
            false,
        )
        .await;
        record_local_request_candidate_status(
            &state,
            &plan,
            usage_payload.report_context.as_ref(),
            SchedulerRequestCandidateStatusUpdate {
                status: if stream_failed {
                    RequestCandidateStatus::Failed
                } else {
                    RequestCandidateStatus::Success
                },
                status_code: Some(status_code),
                error_type: if stream_failed {
                    if missing_observed_finish {
                        Some("stream_missing_terminal_event".to_string())
                    } else {
                        Some("stream_terminal_error".to_string())
                    }
                } else {
                    None
                },
                error_message: stream_failed
                    .then_some(stream_terminal_error_message)
                    .flatten(),
                latency_ms: usage_payload
                    .telemetry
                    .as_ref()
                    .and_then(|value| value.elapsed_ms),
                started_at_unix_ms: Some(candidate_started_unix_secs),
                finished_at_unix_ms: Some(current_request_candidate_unix_ms()),
            },
        )
        .await;

        if should_submit_report {
            if let Err(err) = submit_stream_report(&state, usage_payload).await {
                warn!(
                    event_name = "execution_report_submit_failed",
                    log_type = "ops",
                    trace_id = %trace_id,
                    request_id = %request_id_for_log,
                    candidate_id = ?candidate_id.as_deref(),
                    report_scope = "direct_passthrough_stream",
                    error = ?err,
                    "gateway failed to submit direct passthrough stream execution report"
                );
            }
        }
    }
}

fn build_direct_passthrough_inline_body_stream(
    finalizer: DirectPassthroughFinalizer,
    prefetched_body: VecDeque<Result<Bytes, String>>,
    response: DirectUpstreamResponse,
    upstream_started_at: Instant,
    stream_first_byte_timeout: Option<Duration>,
) -> impl futures_util::Stream<Item = Result<Bytes, IoError>> + Send + 'static {
    let state = DirectPassthroughInlineBodyState::new(
        finalizer,
        prefetched_body,
        response,
        upstream_started_at,
        stream_first_byte_timeout,
    );
    futures_stream::unfold(state, |state| async move { state.next_item().await })
}

struct DirectPassthroughInlineBodyState {
    finalizer: Option<DirectPassthroughFinalizer>,
    upstream: Option<DirectUpstreamByteStream>,
    upstream_control_filter: Option<SseControlBlockFilter>,
    upstream_started_at: Instant,
    stream_first_byte_timeout: Option<Duration>,
    observed_first_body_poll: bool,
    observed_first_client_yield: bool,
    upstream_done: bool,
    control_filter_flushed: bool,
    terminal_error_sent: bool,
    finalized: bool,
}

impl DirectPassthroughInlineBodyState {
    fn new(
        finalizer: DirectPassthroughFinalizer,
        prefetched_body: VecDeque<Result<Bytes, String>>,
        response: DirectUpstreamResponse,
        upstream_started_at: Instant,
        stream_first_byte_timeout: Option<Duration>,
    ) -> Self {
        Self {
            finalizer: Some(finalizer),
            upstream: Some(direct_upstream_response_byte_stream(
                prefetched_body,
                response,
            )),
            upstream_control_filter: Some(SseControlBlockFilter::default()),
            upstream_started_at,
            stream_first_byte_timeout,
            observed_first_body_poll: false,
            observed_first_client_yield: false,
            upstream_done: false,
            control_filter_flushed: false,
            terminal_error_sent: false,
            finalized: false,
        }
    }

    async fn next_item(mut self) -> Option<(Result<Bytes, IoError>, Self)> {
        if self.finalized {
            return None;
        }
        if self
            .finalizer
            .as_ref()
            .is_some_and(DirectPassthroughFinalizer::completed_native_anthropic_stream)
        {
            self.upstream.take();
            self.finalized = true;
            drop(self.finalizer.take());
            return None;
        }
        if !self.observed_first_body_poll {
            self.observed_first_body_poll = true;
            if let Some(finalizer) = self.finalizer.as_mut() {
                finalizer.observe_first_body_poll();
            }
        }
        loop {
            if self.upstream_done
                || self
                    .finalizer
                    .as_ref()
                    .and_then(DirectPassthroughFinalizer::terminal_failure)
                    .is_some()
            {
                break;
            }

            let item = self.next_upstream_item().await;
            let Some(item) = item else {
                self.upstream_done = true;
                break;
            };
            let chunk = match item {
                Ok(chunk) => chunk,
                Err(message) => {
                    self.log_upstream_read_error_and_fail(message);
                    self.upstream_done = true;
                    break;
                }
            };
            if chunk.is_empty() {
                continue;
            }

            let observed_at = Instant::now();
            let Some(chunk) = self
                .finalizer
                .as_mut()
                .and_then(|finalizer| finalizer.prepare_upstream_chunk(chunk))
            else {
                continue;
            };
            let provider_error_detected = if let Some(finalizer) = self.finalizer.as_mut() {
                finalizer.observe_upstream_chunk(&chunk, observed_at);
                finalizer.terminal_failure().is_some()
            } else {
                false
            };
            if let Some(client_chunk) =
                filter_upstream_sse_control_chunk(&mut self.upstream_control_filter, chunk)
            {
                self.prepare_client_chunk_yield(&client_chunk);
                self.terminal_error_sent |= provider_error_detected;
                if self
                    .finalizer
                    .as_ref()
                    .is_some_and(DirectPassthroughFinalizer::completed_native_anthropic_stream)
                {
                    self.upstream.take();
                    self.upstream_done = true;
                }
                return Some((Ok(client_chunk), self));
            }
        }

        if let Some(finalizer) = self.finalizer.as_mut() {
            finalizer.fail_if_anthropic_message_stop_missing();
        }

        if !self.control_filter_flushed
            && self
                .finalizer
                .as_ref()
                .and_then(DirectPassthroughFinalizer::terminal_failure)
                .is_none()
        {
            self.control_filter_flushed = true;
            if let Some(client_chunk) =
                flush_upstream_sse_control_filter(&mut self.upstream_control_filter)
            {
                self.prepare_client_chunk_yield(&client_chunk);
                return Some((Ok(client_chunk), self));
            }
        }

        if !self.terminal_error_sent {
            if let Some(finalizer) = self.finalizer.as_mut() {
                if let Some(failure) = finalizer.terminal_failure() {
                    self.terminal_error_sent = true;
                    match encode_terminal_sse_error_event_for_plan(&finalizer.core().plan, failure)
                    {
                        Ok(error_event) => {
                            self.prepare_client_chunk_yield(&error_event);
                            return Some((Ok(error_event), self));
                        }
                        Err(err) => finalizer.log_terminal_error_event_encode_failed(err),
                    }
                }
            }
        }

        self.finalize(false).await;
        None
    }

    async fn next_upstream_item(&mut self) -> Option<Result<Bytes, String>> {
        let needs_first_byte_timeout = self
            .finalizer
            .as_ref()
            .is_some_and(|finalizer| !finalizer.ttfb_observed());
        let upstream = self.upstream.as_mut()?;
        if needs_first_byte_timeout {
            match await_direct_passthrough_first_item(
                upstream.next(),
                self.upstream_started_at,
                self.stream_first_byte_timeout,
            )
            .await
            {
                Ok(item) => item,
                Err(timeout) => {
                    if let Some(finalizer) = self.finalizer.as_mut() {
                        finalizer.set_terminal_failure(build_stream_transport_failure_report(
                            "first_byte_timeout",
                            stream_first_byte_timeout_message(timeout),
                            504,
                        ));
                    }
                    None
                }
            }
        } else {
            upstream.next().await
        }
    }

    fn prepare_client_chunk_yield(&mut self, chunk: &Bytes) {
        let Some(finalizer) = self.finalizer.as_mut() else {
            return;
        };
        finalizer.observe_client_chunk(chunk);
        if !self.observed_first_client_yield {
            self.observed_first_client_yield = true;
            finalizer.release_upstream_target_permit_after_first_yield();
            finalizer.record_client_visible_stream_started_if_needed();
            finalizer.observe_first_client_yield();
        }
    }

    fn log_upstream_read_error_and_fail(&mut self, message: String) {
        let Some(finalizer) = self.finalizer.as_mut() else {
            return;
        };
        if finalizer.completed_native_anthropic_stream() {
            let core = finalizer.core();
            debug!(
                event_name = "direct_passthrough_read_error_ignored_after_anthropic_stop",
                log_type = "debug",
                trace_id = %core.trace_id,
                request_id = %core.request_id_for_log,
                candidate_id = ?core.candidate_id.as_deref(),
                error = %message,
                "gateway ignored direct passthrough teardown error after Anthropic message_stop"
            );
            return;
        }
        let core = finalizer.core();
        warn!(
            event_name = "direct_passthrough_body_read_error",
            log_type = "ops",
            trace_id = %core.trace_id,
            request_id = %core.request_id_for_log,
            candidate_id = ?core.candidate_id.as_deref(),
            upstream_bytes = core.provider_stream_bytes,
            error = %message,
            "gateway direct passthrough upstream body read failed"
        );
        finalizer.set_terminal_failure(build_stream_transport_failure_report(
            "execution_runtime_stream_read_error",
            message,
            502,
        ));
    }

    async fn finalize(&mut self, downstream_dropped: bool) {
        if self.finalized {
            return;
        }
        self.finalized = true;
        self.upstream.take();
        if let Some(finalizer) = self.finalizer.as_mut() {
            finalizer.finalize(downstream_dropped).await;
        }
        self.finalizer.take();
    }
}

impl Drop for DirectPassthroughInlineBodyState {
    fn drop(&mut self) {
        // `finalized` only prevents another poll from entering finalization;
        // the finalizer may still be waiting for its handoff task to finish.
        // Keep the fallback armed while the finalizer is present.
        if self.finalizer.is_none() {
            return;
        }
        self.upstream.take();
        if let Some(finalizer) = self.finalizer.take() {
            observe_gateway_stage_ms("stream_finalizer_enqueue", 0);
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.spawn(async move {
                    let mut finalizer = finalizer;
                    finalizer.finalize(true).await;
                });
            }
        }
    }
}

async fn record_stream_pending_lifecycle(
    state: &AppState,
    lifecycle_seed: &LifecycleUsageSeed,
    stage_trace: &mut RequestStageTrace,
) {
    let usage_pending_started_at = Instant::now();
    let usage_data = state.usage_lifecycle_data_state().as_ref().clone();
    state
        .usage_runtime
        .record_pending_direct(&usage_data, lifecycle_seed.clone())
        .await;
    observe_gateway_stage_trace_ms(
        stage_trace,
        "stream_usage_pending",
        usage_pending_started_at.elapsed().as_millis() as u64,
    );
}

fn should_defer_stream_pending_for_direct_inline(
    state: &AppState,
    plan: &ExecutionPlan,
    plan_kind: &str,
    report_context: Option<&Value>,
) -> bool {
    #[cfg(test)]
    if state
        .execution_runtime_override_base_url()
        .is_some_and(|value| !value.trim().is_empty())
    {
        return false;
    }
    if !plan
        .provider_api_format
        .eq_ignore_ascii_case(plan.client_api_format.as_str())
    {
        return false;
    }
    if client_format_allows_proxy_generated_sse_control_blocks(plan) {
        return false;
    }
    let _ = (state, plan_kind, report_context);
    true
}

#[allow(clippy::too_many_arguments)]
async fn execute_stream_from_direct_passthrough(
    state: &AppState,
    plan: ExecutionPlan,
    trace_id: &str,
    decision: &GatewayControlDecision,
    plan_kind: &str,
    report_kind: Option<String>,
    report_context: Option<serde_json::Value>,
    candidate_started_unix_secs: u64,
    stream_started_at: Instant,
    mut stage_trace: RequestStageTrace,
    execution: DirectUpstreamStreamExecution,
    pending_recorded: bool,
) -> Result<Option<Response<Body>>, GatewayError> {
    let DirectUpstreamStreamExecution {
        request_id: _,
        candidate_id: _,
        status_code,
        mut headers,
        provider_api_format: _,
        stream_summary_report_context: _,
        prefetched_body,
        stream_precommit_committed: _,
        response,
        started_at: upstream_started_at,
        response_observation,
        stream_first_byte_timeout,
        upstream_target_permit,
    } = execution;

    let requires_anthropic_message_stop = status_code == 200
        && response_headers_indicate_sse(&headers)
        && plan
            .provider_api_format
            .eq_ignore_ascii_case("claude:messages")
        && plan
            .client_api_format
            .eq_ignore_ascii_case("claude:messages");

    let request_id = plan.request_id.clone();
    let candidate_id = plan.candidate_id.clone();
    let request_id_for_log = short_request_id(request_id.as_str());
    let mut report_context = attach_provider_response_headers_to_report_context(
        report_context,
        &headers,
        response_observation.request_started_at_unix_ms,
        response_observation.response_headers_observed_at_unix_ms,
        &response_observation.request_order_id,
    );

    let lifecycle_seed = build_lifecycle_usage_seed(&plan, report_context.as_ref());
    let max_stream_body_buffer_bytes = resolve_stream_body_buffer_limit(state).await;
    let request_candidate_status_snapshot =
        snapshot_local_request_candidate_status(&plan, report_context.as_ref());

    let response_header_rules_started_at = Instant::now();
    apply_endpoint_response_header_rules(state, &plan, &mut headers, None).await?;
    observe_gateway_stage_ms(
        "stream_response_header_rules",
        response_header_rules_started_at.elapsed().as_millis() as u64,
    );
    let headers_for_report = headers.clone();
    headers.insert(CONTROL_REQUEST_ID_HEADER.to_string(), request_id.clone());
    if let Some(candidate_id) = candidate_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        headers.insert(
            CONTROL_CANDIDATE_ID_HEADER.to_string(),
            candidate_id.to_string(),
        );
    }
    headers.remove("content-length");

    let direct_stream_finalize_kind = resolve_core_stream_direct_finalize_report_kind(plan_kind);
    let stream_usage_report_context = report_context.clone().or_else(|| {
        Some(serde_json::json!({
            "provider_api_format": plan.provider_api_format.as_str(),
            "client_api_format": plan.client_api_format.as_str(),
        }))
    });
    let stream_usage_observer = stream_usage_report_context
        .as_ref()
        .map(|_| StreamingStandardTerminalObserver::default());
    observe_gateway_stage_trace_ms(
        &mut stage_trace,
        "stream_response_ready",
        stream_elapsed_ms_since(stream_started_at),
    );
    let request_diagnostics = current_request_diagnostics();
    observe_request_accepted_stage_trace_ms(
        &mut stage_trace,
        request_diagnostics.as_ref(),
        "frontdoor_to_stream_response_ready",
    );
    let finalizer = DirectPassthroughFinalizer::new(DirectPassthroughFinalizerCore {
        state: state.clone(),
        plan,
        trace_id: trace_id.to_string(),
        report_kind,
        report_context,
        lifecycle_seed,
        direct_stream_finalize_kind,
        stream_started_at,
        stage_trace,
        request_diagnostics,
        request_id_for_log,
        candidate_id,
        request_candidate_status_snapshot,
        deferred_request_candidate_status_record: None,
        candidate_started_unix_secs,
        status_code,
        headers: headers_for_report,
        stream_usage_report_context,
        stream_usage_observer,
        stream_usage_observer_buffered: Vec::new(),
        max_stream_body_buffer_bytes,
        provider_buffered_body: Vec::new(),
        buffered_body: Vec::new(),
        provider_body_truncated: false,
        client_body_truncated: false,
        client_stream_completion_tracker: ClientVisibleStreamCompletionTracker::default(),
        requires_anthropic_message_stop,
        client_visible_stream_completed: false,
        usage_stream_telemetry: None,
        telemetry: None,
        provider_stream_bytes: 0,
        client_stream_bytes: 0,
        last_client_chunk_elapsed_ms: 0,
        pending_recorded,
        stream_started_recorded: false,
        terminal_failure: None,
        _upstream_target_permit: upstream_target_permit,
    });
    let body_stream = build_direct_passthrough_inline_body_stream(
        finalizer,
        prefetched_body,
        response,
        upstream_started_at,
        stream_first_byte_timeout,
    );
    return Ok(Some(build_client_response_from_parts(
        status_code,
        &headers,
        Body::from_stream(body_stream),
        trace_id,
        Some(decision),
    )?));
}

#[allow(clippy::too_many_arguments)] // internal function, grouping would add unnecessary indirection
pub(crate) fn execute_execution_runtime_stream<'a>(
    state: &'a AppState,
    plan: ExecutionPlan,
    trace_id: &'a str,
    decision: &'a GatewayControlDecision,
    plan_kind: &'a str,
    report_kind: Option<String>,
    report_context: Option<serde_json::Value>,
) -> Pin<Box<dyn Future<Output = Result<Option<Response<Body>>, GatewayError>> + Send + 'a>> {
    Box::pin(execute_execution_runtime_stream_inner(
        state,
        plan,
        trace_id,
        decision,
        plan_kind,
        report_kind,
        report_context,
        None,
        None,
    ))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn execute_execution_runtime_stream_with_retry_scope<'a>(
    state: &'a AppState,
    plan: ExecutionPlan,
    trace_id: &'a str,
    decision: &'a GatewayControlDecision,
    plan_kind: &'a str,
    report_kind: Option<String>,
    report_context: Option<serde_json::Value>,
) -> Pin<
    Box<
        dyn Future<Output = Result<AiAttemptExecutionOutcome<Response<Body>>, GatewayError>>
            + Send
            + 'a,
    >,
> {
    Box::pin(async move {
        let mut retry_scope = AiAttemptRetryScope::Candidate;
        let mut fallback_response = None;
        let response = execute_execution_runtime_stream_inner(
            state,
            plan,
            trace_id,
            decision,
            plan_kind,
            report_kind,
            report_context,
            Some(&mut retry_scope),
            Some(&mut fallback_response),
        )
        .await?;
        Ok(match response {
            Some(response) => AiAttemptExecutionOutcome::Responded(response),
            None => AiAttemptExecutionOutcome::Retry {
                scope: retry_scope,
                fallback_response,
            },
        })
    })
}

async fn maybe_build_stream_transport_error_stop_response(
    state: &AppState,
    plan: &ExecutionPlan,
    report_context: Option<&Value>,
    trace_id: &str,
    decision: &GatewayControlDecision,
    error_type: &str,
    error_message: &str,
    elapsed_ms: u64,
) -> Result<Option<Response<Body>>, GatewayError> {
    let analysis = crate::orchestration::resolve_local_transport_failover_analysis_for_attempt(
        state,
        plan,
        report_context,
    )
    .await;
    if !matches!(analysis.decision, LocalFailoverDecision::StopLocalFailover) {
        return Ok(None);
    }

    crate::execution_runtime::build_transport_error_stop_response(
        state,
        plan,
        report_context,
        trace_id,
        decision,
        http::StatusCode::BAD_GATEWAY.as_u16(),
        error_type,
        error_message,
        elapsed_ms,
    )
    .await
    .map(Some)
}

async fn execute_execution_runtime_stream_inner(
    state: &AppState,
    mut plan: ExecutionPlan,
    trace_id: &str,
    decision: &GatewayControlDecision,
    plan_kind: &str,
    report_kind: Option<String>,
    mut report_context: Option<serde_json::Value>,
    mut retry_scope_out: Option<&mut AiAttemptRetryScope>,
    mut retry_fallback_out: Option<&mut Option<Response<Body>>>,
) -> Result<Option<Response<Body>>, GatewayError> {
    let stream_started_at = Instant::now();
    let mut stage_trace = RequestStageTrace::from_env();
    let candidate_slot_started_at = Instant::now();
    ensure_execution_request_candidate_slot(state, &mut plan, &mut report_context).await;
    observe_gateway_stage_trace_ms(
        &mut stage_trace,
        "stream_candidate_slot",
        candidate_slot_started_at.elapsed().as_millis() as u64,
    );
    let request_candidate_status_snapshot =
        snapshot_local_request_candidate_status(&plan, report_context.as_ref());
    let defer_stream_pending_for_direct_inline = should_defer_stream_pending_for_direct_inline(
        state,
        &plan,
        plan_kind,
        report_context.as_ref(),
    );
    // Inline passthrough records its lifecycle seed after upstream headers are
    // available. Avoid constructing a throwaway seed on the common path.
    let mut lifecycle_seed = (!defer_stream_pending_for_direct_inline)
        .then(|| build_lifecycle_usage_seed(&plan, report_context.as_ref()));
    let mut lifecycle_pending_recorded = false;
    if let Some(seed) = lifecycle_seed.as_ref() {
        record_stream_pending_lifecycle(state, seed, &mut stage_trace).await;
        lifecycle_pending_recorded = true;
    }
    let candidate_started_unix_secs = current_request_candidate_unix_ms();
    if let Some(snapshot) = request_candidate_status_snapshot.clone() {
        record_local_request_candidate_status_snapshot(
            state,
            &snapshot,
            SchedulerRequestCandidateStatusUpdate {
                status: RequestCandidateStatus::Pending,
                status_code: None,
                error_type: None,
                error_message: None,
                latency_ms: None,
                started_at_unix_ms: Some(candidate_started_unix_secs),
                finished_at_unix_ms: None,
            },
        )
        .await;
    }
    let plan_request_id_for_log = short_request_id(plan.request_id.as_str());
    let provider_name = plan
        .provider_name
        .clone()
        .unwrap_or_else(|| "-".to_string());
    let endpoint_id = plan.endpoint_id.clone();
    let key_id = plan.key_id.clone();
    let model_name = plan.model_name.clone().unwrap_or_else(|| "-".to_string());
    let candidate_index = parse_request_candidate_report_context(report_context.as_ref())
        .and_then(|context| context.candidate_index)
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_string());
    let _ = (&mut retry_scope_out, &mut retry_fallback_out);
    let upstream_headers_started_at = Instant::now();
    let execution = match execute_in_process_stream_with_report_context(
        state,
        &plan,
        trace_id,
        report_context.as_ref(),
    )
    .await
    {
        Ok(execution) => execution,
        Err(InProcessStreamExecutionError::Gateway(err)) => {
            if matches!(err, GatewayError::AdmissionTimeout { .. }) {
                record_stream_admission_timeout_candidate_failure(
                    state,
                    &plan,
                    report_context.as_ref(),
                    candidate_started_unix_secs,
                    &err,
                )
                .await;
            }
            return Err(err);
        }
        Err(InProcessStreamExecutionError::Transport(err)) => {
            let transport_error_message = err.to_string();
            info!(
                event_name = "stream_execution_runtime_unavailable",
                log_type = "ops",
                trace_id = %trace_id,
                request_id = %plan_request_id_for_log,
                candidate_id = ?plan.candidate_id,
                provider_name,
                endpoint_id,
                key_id,
                model_name,
                candidate_index = candidate_index.as_str(),
                error = %err,
                "gateway in-process stream execution unavailable"
            );
            let terminal_unix_secs = current_request_candidate_unix_ms();
            record_local_request_candidate_status(
                state,
                &plan,
                report_context.as_ref(),
                SchedulerRequestCandidateStatusUpdate {
                    status: RequestCandidateStatus::Failed,
                    status_code: None,
                    error_type: Some("execution_runtime_unavailable".to_string()),
                    error_message: Some(transport_error_message.clone()),
                    latency_ms: Some(stream_elapsed_ms_since(stream_started_at)),
                    started_at_unix_ms: Some(candidate_started_unix_secs),
                    finished_at_unix_ms: Some(terminal_unix_secs),
                },
            )
            .await;
            if let Some(response) = maybe_build_stream_transport_error_stop_response(
                state,
                &plan,
                report_context.as_ref(),
                trace_id,
                decision,
                "execution_runtime_unavailable",
                transport_error_message.as_str(),
                stream_elapsed_ms_since(stream_started_at),
            )
            .await?
            {
                return Ok(Some(response));
            }
            return Ok(None);
        }
    };
    observe_gateway_stage_trace_ms(
        &mut stage_trace,
        "stream_upstream_headers",
        upstream_headers_started_at.elapsed().as_millis() as u64,
    );
    Box::pin(execute_stream_from_direct_passthrough(
        state,
        plan,
        trace_id,
        decision,
        plan_kind,
        report_kind,
        report_context,
        candidate_started_unix_secs,
        stream_started_at,
        stage_trace,
        execution,
        lifecycle_pending_recorded,
    ))
    .await
}

fn decode_stream_data_chunk(
    chunk_b64: Option<&str>,
    text: Option<&str>,
) -> Result<Vec<u8>, GatewayError> {
    if let Some(chunk_b64) = chunk_b64 {
        return base64::engine::general_purpose::STANDARD
            .decode(chunk_b64)
            .map_err(|err| GatewayError::Internal(err.to_string()));
    }
    Ok(text.unwrap_or_default().as_bytes().to_vec())
}

fn response_headers_indicate_sse(headers: &BTreeMap<String, String>) -> bool {
    headers
        .get("content-type")
        .map(String::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some_and(|value| value.to_ascii_lowercase().contains("text/event-stream"))
}

fn parse_prefetched_sync_json_body(body: &[u8]) -> Option<Value> {
    let stripped = strip_utf8_bom_and_ws(body);
    serde_json::from_slice::<Value>(stripped).ok()
}

fn anthropic_premature_eof_error_body(message: &str) -> Value {
    serde_json::json!({
        "type": "error",
        "error": {
            "type": "api_error",
            "message": message,
        }
    })
}

fn build_anthropic_premature_eof_failure(message: &str) -> StreamFailureReport {
    let body_json = anthropic_premature_eof_error_body(message);
    build_stream_failure_from_provider_error_body(
        anthropic_error_status_code(&body_json),
        &body_json,
    )
}

fn encode_terminal_sse_error_event(failure: &StreamFailureReport) -> Result<Bytes, std::io::Error> {
    let payload = failure
        .to_json_string()
        .map_err(|err| IoError::other(err.to_string()))?;
    let mut event = String::new();
    for line in payload.lines() {
        event.push_str("data: ");
        event.push_str(line);
        event.push('\n');
    }
    event.push_str("\ndata: [DONE]\n\n");
    Ok(Bytes::from(event))
}

fn encode_anthropic_terminal_sse_error_event(
    failure: &StreamFailureReport,
) -> Result<Bytes, std::io::Error> {
    let payload = serde_json::to_string(&serde_json::json!({
        "type": "error",
        "error": {
            "type": "api_error",
            "message": failure.error_message,
        }
    }))
    .map_err(|err| IoError::other(err.to_string()))?;
    Ok(Bytes::from(format!("event: error\ndata: {payload}\n\n")))
}

fn encode_terminal_sse_error_event_for_plan(
    plan: &ExecutionPlan,
    failure: &StreamFailureReport,
) -> Result<Bytes, std::io::Error> {
    if plan
        .client_api_format
        .trim()
        .eq_ignore_ascii_case("claude:messages")
        && plan
            .provider_api_format
            .trim()
            .eq_ignore_ascii_case("claude:messages")
    {
        encode_anthropic_terminal_sse_error_event(failure)
    } else {
        encode_terminal_sse_error_event(failure)
    }
}

fn image_stream_failed_event_name(report_context: Option<&Value>) -> &'static str {
    let operation = report_context
        .and_then(|value| value.get("image_request"))
        .and_then(|value| value.get("operation"))
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    if operation == "edit" {
        "image_edit.failed"
    } else {
        "image_generation.failed"
    }
}

fn encode_openai_image_failed_event(
    report_context: Option<&Value>,
    failure: &StreamFailureReport,
) -> Result<Bytes, std::io::Error> {
    let event_name = image_stream_failed_event_name(report_context);
    let failure_body = failure
        .to_json_string()
        .map_err(|err| IoError::other(err.to_string()))?;
    let failure_json: Value =
        serde_json::from_str(&failure_body).map_err(|err| IoError::other(err.to_string()))?;
    let error = failure_json.get("error").cloned().unwrap_or_else(|| {
        serde_json::json!({
            "type": failure.error_type.as_str(),
            "message": failure.error_message.as_str(),
            "code": failure.status_code,
        })
    });
    let payload = serde_json::json!({
        "type": event_name,
        "error": error,
    });
    let payload = serde_json::to_string(&payload).map_err(|err| IoError::other(err.to_string()))?;
    let mut event = format!("event: {event_name}\n");
    for line in payload.lines() {
        event.push_str("data: ");
        event.push_str(line);
        event.push('\n');
    }
    event.push('\n');
    Ok(Bytes::from(event))
}

fn should_limit_direct_finalize_prefetch(plan_kind: &str, has_local_stream_rewriter: bool) -> bool {
    plan_kind == OPENAI_IMAGE_STREAM_PLAN_KIND || has_local_stream_rewriter
}

fn client_format_allows_proxy_generated_sse_control_blocks(plan: &ExecutionPlan) -> bool {
    // OpenAI-compatible clients commonly parse every client-visible SSE event as
    // an OpenAI JSON payload or [DONE]. Keep the downstream wire format strict:
    // do not inject proxy-generated comments, pings, or keepalives for openai:*.
    !plan
        .client_api_format
        .trim()
        .to_ascii_lowercase()
        .starts_with("openai:")
}

fn truncate_at_anthropic_message_stop(
    tracker: Option<&mut ClientVisibleStreamCompletionTracker>,
    chunk: &mut Bytes,
) -> bool {
    let Some(tracker) = tracker else {
        return false;
    };
    if let Some(terminal_end) = tracker.observe_anthropic_message_stop_terminal_end(chunk.as_ref())
    {
        chunk.truncate(terminal_end);
        return true;
    }
    false
}

#[derive(Default)]
struct SseControlBlockFilter {
    buffered: Vec<u8>,
    emitted_len: usize,
    passthrough_current_block: bool,
}

impl SseControlBlockFilter {
    fn push_chunk(&mut self, chunk: &[u8]) -> Vec<u8> {
        if chunk.is_empty() {
            return Vec::new();
        }

        self.buffered.extend_from_slice(chunk);
        let mut output = Vec::new();
        while let Some((block_end, separator_len)) = find_sse_block_boundary(&self.buffered) {
            let block_len = block_end + separator_len;
            let block = self.buffered.drain(..block_len).collect::<Vec<_>>();
            if self.passthrough_current_block {
                let emitted_len = self.emitted_len.min(block.len());
                output.extend_from_slice(&block[emitted_len..]);
            } else if sse_block_has_data_line(&block) {
                output.extend_from_slice(&block);
            }
            self.emitted_len = 0;
            self.passthrough_current_block = false;
        }

        if self.passthrough_current_block {
            if self.buffered.len() > self.emitted_len {
                output.extend_from_slice(&self.buffered[self.emitted_len..]);
                self.emitted_len = self.buffered.len();
            }
        } else if sse_buffer_has_data_line(&self.buffered) {
            self.passthrough_current_block = true;
            output.extend_from_slice(&self.buffered);
            self.emitted_len = self.buffered.len();
        }

        if self.buffered.len() > SSE_CONTROL_FILTER_MAX_BUFFER_BYTES {
            let buffered = std::mem::take(&mut self.buffered);
            if self.passthrough_current_block {
                let emitted_len = self.emitted_len.min(buffered.len());
                output.extend_from_slice(&buffered[emitted_len..]);
            } else {
                output.extend(buffered);
            }
            self.emitted_len = 0;
            self.passthrough_current_block = false;
        }

        output
    }

    fn finish(&mut self) -> Vec<u8> {
        if self.buffered.is_empty() {
            return Vec::new();
        }

        let block = std::mem::take(&mut self.buffered);
        let emitted_len = self.emitted_len.min(block.len());
        let passthrough_current_block = self.passthrough_current_block;
        self.emitted_len = 0;
        self.passthrough_current_block = false;
        if passthrough_current_block {
            block[emitted_len..].to_vec()
        } else if sse_block_has_data_line(&block) {
            block
        } else {
            Vec::new()
        }
    }
}

fn filter_upstream_sse_control_chunk(
    filter: &mut Option<SseControlBlockFilter>,
    chunk: Bytes,
) -> Option<Bytes> {
    let Some(filter) = filter.as_mut() else {
        return Some(chunk);
    };

    let filtered = filter.push_chunk(chunk.as_ref());
    (!filtered.is_empty()).then(|| Bytes::from(filtered))
}

fn flush_upstream_sse_control_filter(filter: &mut Option<SseControlBlockFilter>) -> Option<Bytes> {
    let filtered = filter.as_mut()?.finish();
    (!filtered.is_empty()).then(|| Bytes::from(filtered))
}

fn find_sse_block_boundary(buffer: &[u8]) -> Option<(usize, usize)> {
    find_sse_record_boundary(buffer)
}

fn sse_block_has_data_line(block: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(block) else {
        return true;
    };

    text.split(['\r', '\n'])
        .any(|line| line.trim_start().starts_with("data:"))
}

fn sse_buffer_has_data_line(buffer: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(buffer) else {
        return true;
    };

    text.split(['\r', '\n'])
        .any(|line| line.trim_start().starts_with("data:"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SseTerminalPolicy {
    AnyKnown,
    AnthropicMessageStop,
}

#[derive(Default)]
struct ClientVisibleStreamCompletionTracker {
    line_buffer: Vec<u8>,
    event_type: Option<String>,
    data_payload: String,
    has_data_payload: bool,
    record_bytes: usize,
    dropping_oversized_record: bool,
    discarded_line_nonempty: bool,
    skip_next_lf: bool,
    completed: bool,
}

impl ClientVisibleStreamCompletionTracker {
    fn observe_chunk(&mut self, chunk: &[u8]) -> bool {
        self.observe_chunk_terminal_end(chunk);
        self.completed
    }

    fn observe_chunk_terminal_end(&mut self, chunk: &[u8]) -> Option<usize> {
        self.observe_chunk_terminal_end_with_policy(chunk, SseTerminalPolicy::AnyKnown)
    }

    fn observe_anthropic_message_stop(&mut self, chunk: &[u8]) -> bool {
        self.observe_anthropic_message_stop_terminal_end(chunk);
        self.completed
    }

    fn observe_anthropic_message_stop_terminal_end(&mut self, chunk: &[u8]) -> Option<usize> {
        self.observe_chunk_terminal_end_with_policy(chunk, SseTerminalPolicy::AnthropicMessageStop)
    }

    fn observe_chunk_terminal_end_with_policy(
        &mut self,
        chunk: &[u8],
        policy: SseTerminalPolicy,
    ) -> Option<usize> {
        if self.completed {
            return None;
        }
        if chunk.is_empty() {
            return None;
        }

        for (index, byte) in chunk.iter().enumerate() {
            self.record_bytes = self.record_bytes.saturating_add(1);
            if self.record_bytes > SSE_TERMINAL_DETECTOR_MAX_RECORD_BYTES
                && !self.dropping_oversized_record
            {
                self.dropping_oversized_record = true;
                self.discarded_line_nonempty = !self.line_buffer.is_empty();
                self.line_buffer.clear();
                self.reset_current_event();
            }

            if self.skip_next_lf {
                self.skip_next_lf = false;
                if *byte == b'\n' {
                    continue;
                }
            }

            if self.dropping_oversized_record {
                match *byte {
                    b'\n' => self.finish_discarded_line(),
                    b'\r' => {
                        self.finish_discarded_line();
                        self.skip_next_lf = true;
                    }
                    _ => self.discarded_line_nonempty = true,
                }
                continue;
            }

            match *byte {
                b'\n' => self.finish_line(policy),
                b'\r' => {
                    self.finish_line(policy);
                    self.skip_next_lf = true;
                }
                _ => self.line_buffer.push(*byte),
            }

            if self.completed {
                let terminal_end = if *byte == b'\r' && chunk.get(index + 1) == Some(&b'\n') {
                    index + 2
                } else {
                    index + 1
                };
                return Some(terminal_end);
            }
        }

        None
    }

    fn finish_line(&mut self, policy: SseTerminalPolicy) {
        let line = std::mem::take(&mut self.line_buffer);
        let Ok(line) = std::str::from_utf8(&line) else {
            self.reset_current_event();
            return;
        };
        let line = line.trim();

        if line.is_empty() {
            self.completed = self.current_event_is_terminal(policy);
            self.reset_current_event();
            self.record_bytes = 0;
            return;
        }

        if let Some(event_type) = line.strip_prefix("event:").map(str::trim) {
            self.event_type = Some(event_type.to_string());
            return;
        }

        if let Some(data) = line.strip_prefix("data:").map(str::trim) {
            if data.is_empty() {
                return;
            }
            if self.has_data_payload {
                self.data_payload.push('\n');
            }
            self.data_payload.push_str(data);
            self.has_data_payload = true;
        }
    }

    fn finish_discarded_line(&mut self) {
        if !self.discarded_line_nonempty {
            self.dropping_oversized_record = false;
            self.record_bytes = 0;
            self.reset_current_event();
        }
        self.discarded_line_nonempty = false;
    }

    fn current_event_is_terminal(&self, policy: SseTerminalPolicy) -> bool {
        match policy {
            SseTerminalPolicy::AnyKnown => {
                self.event_type
                    .as_deref()
                    .is_some_and(is_terminal_sse_event_type)
                    || (self.has_data_payload && sse_data_payload_is_terminal(&self.data_payload))
            }
            SseTerminalPolicy::AnthropicMessageStop => {
                let payload_type = self
                    .has_data_payload
                    .then(|| serde_json::from_str::<serde_json::Value>(&self.data_payload).ok())
                    .flatten()
                    .and_then(|value| {
                        value
                            .get("type")
                            .and_then(serde_json::Value::as_str)
                            .map(ToOwned::to_owned)
                    });
                payload_type.as_deref() == Some("message_stop")
                    && self
                        .event_type
                        .as_deref()
                        .is_none_or(|event_type| event_type == "message_stop")
            }
        }
    }

    fn reset_current_event(&mut self) {
        self.event_type = None;
        self.data_payload.clear();
        self.has_data_payload = false;
    }
}

fn is_terminal_sse_event_type(event_type: &str) -> bool {
    matches!(
        event_type,
        "message_stop" | "response.completed" | "response.failed" | "response.incomplete" | "error"
    )
}

fn sse_data_payload_is_terminal(data: &str) -> bool {
    data == "[DONE]"
        || serde_json::from_str::<serde_json::Value>(data).is_ok_and(|value| {
            value
                .get("type")
                .and_then(serde_json::Value::as_str)
                .is_some_and(is_terminal_sse_event_type)
        })
}

fn stream_chunk_contains_sse_done(chunk: &[u8]) -> bool {
    let mut tracker = ClientVisibleStreamCompletionTracker::default();
    tracker.observe_chunk(chunk)
}

struct ObservedStreamFrame {
    frame: StreamFrame,
    observed_at: Instant,
}

#[derive(Clone)]
struct PostStopFrameReadBudget {
    remaining: Arc<AtomicUsize>,
}

impl PostStopFrameReadBudget {
    fn new() -> Self {
        Self {
            remaining: Arc::new(AtomicUsize::new(POST_STOP_FRAME_READ_BUDGET_INACTIVE)),
        }
    }

    fn activate(&self, already_buffered: usize) -> bool {
        let remaining = ANTHROPIC_POST_STOP_DRAIN_MAX_BYTES.saturating_sub(already_buffered);
        let activated = self
            .remaining
            .compare_exchange(
                POST_STOP_FRAME_READ_BUDGET_INACTIVE,
                remaining,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok();
        activated && already_buffered > ANTHROPIC_POST_STOP_DRAIN_MAX_BYTES
    }
}

struct PostStopLimitedStreamReader<S> {
    stream: S,
    current: Option<Bytes>,
    budget: PostStopFrameReadBudget,
}

impl<S> PostStopLimitedStreamReader<S> {
    fn new(stream: S, budget: PostStopFrameReadBudget) -> Self {
        Self {
            stream,
            current: None,
            budget,
        }
    }

    fn activate_post_stop_budget(&mut self, already_buffered: usize) -> bool {
        let over_limit = self.budget.activate(already_buffered);
        let remaining = self.budget.remaining.load(Ordering::Acquire);
        self.trim_current_to_budget(remaining, true);
        over_limit
    }

    fn trim_current_to_budget(&mut self, remaining: usize, detach_backing: bool) {
        if remaining == POST_STOP_FRAME_READ_BUDGET_INACTIVE {
            return;
        }
        if remaining == 0 {
            self.current = None;
            return;
        }
        if let Some(current) = self.current.as_mut() {
            if detach_backing || current.len() > remaining {
                let retained = current.len().min(remaining);
                // Detach even a small slice because it can retain a giant
                // producer allocation across post-stop backpressure.
                *current = Bytes::copy_from_slice(&current[..retained]);
            }
        }
    }
}

impl<S> AsyncRead for PostStopLimitedStreamReader<S>
where
    S: Stream<Item = Result<Bytes, IoError>> + Unpin,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<Result<(), IoError>> {
        let this = self.get_mut();
        if buf.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }

        let mut empty_chunks = 0usize;
        loop {
            let remaining = this.budget.remaining.load(Ordering::Acquire);
            if remaining == 0 {
                this.current = None;
                return Poll::Ready(Ok(()));
            }
            this.trim_current_to_budget(remaining, false);

            if let Some(current) = this.current.as_mut() {
                let read = current.len().min(buf.remaining());
                if read > 0 {
                    buf.put_slice(&current.split_to(read));
                    if remaining != POST_STOP_FRAME_READ_BUDGET_INACTIVE {
                        let previous = this.budget.remaining.fetch_sub(read, Ordering::AcqRel);
                        debug_assert!(previous != POST_STOP_FRAME_READ_BUDGET_INACTIVE);
                        debug_assert!(previous >= read);
                    }
                }
                if current.is_empty() {
                    this.current = None;
                }
                if read > 0 {
                    return Poll::Ready(Ok(()));
                }
            }

            match Pin::new(&mut this.stream).poll_next(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Some(Ok(chunk))) if chunk.is_empty() => {
                    empty_chunks += 1;
                    if empty_chunks >= POST_STOP_MAX_EMPTY_CHUNKS_PER_POLL {
                        cx.waker().wake_by_ref();
                        return Poll::Pending;
                    }
                }
                Poll::Ready(Some(Ok(chunk))) => {
                    this.current = Some(chunk);
                    if remaining != POST_STOP_FRAME_READ_BUDGET_INACTIVE {
                        this.trim_current_to_budget(remaining, true);
                    }
                }
                Poll::Ready(Some(Err(err))) => return Poll::Ready(Err(err)),
                Poll::Ready(None) => return Poll::Ready(Ok(())),
            }
        }
    }
}

fn activate_post_stop_frame_read_budget<S>(
    lines: &mut FramedRead<PostStopLimitedStreamReader<S>, LinesCodec>,
) -> bool {
    let already_buffered = lines.read_buffer().len();
    let over_limit = already_buffered > ANTHROPIC_POST_STOP_DRAIN_MAX_BYTES;
    let reader_over_limit = lines.get_mut().activate_post_stop_budget(already_buffered);
    let retained = if over_limit { 0 } else { already_buffered };
    let mut bounded = bytes::BytesMut::with_capacity(retained);
    bounded.extend_from_slice(&lines.read_buffer()[..retained]);
    *lines.read_buffer_mut() = bounded;
    reader_over_limit || over_limit
}

async fn read_next_observed_stream_frame<R>(
    lines: &mut FramedRead<R, LinesCodec>,
) -> Result<Option<ObservedStreamFrame>, GatewayError>
where
    R: tokio::io::AsyncRead + Unpin,
{
    Ok(read_next_frame(lines)
        .await?
        .map(|frame| ObservedStreamFrame {
            frame,
            observed_at: Instant::now(),
        }))
}

async fn next_stream_frame<R>(
    buffered_frames: &mut VecDeque<ObservedStreamFrame>,
    lines: &mut FramedRead<R, LinesCodec>,
) -> Result<Option<ObservedStreamFrame>, GatewayError>
where
    R: tokio::io::AsyncRead + Unpin,
{
    if let Some(frame) = buffered_frames.pop_front() {
        return Ok(Some(frame));
    }
    read_next_observed_stream_frame(lines).await
}

fn serialized_stream_frame_len(frame: &StreamFrame) -> usize {
    serde_json::to_vec(frame).map_or(usize::MAX, |encoded| encoded.len())
}

fn should_refresh_stream_usage_telemetry(
    previous: Option<&ExecutionTelemetry>,
    next: &ExecutionTelemetry,
) -> bool {
    let previous_ttfb = previous.and_then(|telemetry| telemetry.ttfb_ms);
    let previous_elapsed = previous.and_then(|telemetry| telemetry.elapsed_ms);
    let next_ttfb = next.ttfb_ms;
    let next_elapsed = next.elapsed_ms;

    (next_ttfb.is_some() && next_ttfb != previous_ttfb)
        || (next_elapsed.is_some() && next_elapsed != previous_elapsed)
}

fn stream_elapsed_ms_since(started_at: Instant) -> u64 {
    started_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}

fn stream_elapsed_ms_at(started_at: Instant, observed_at: Instant) -> u64 {
    observed_at
        .saturating_duration_since(started_at)
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn first_stream_event_telemetry(
    stream_started_at: Instant,
    event_observed_at: Instant,
    upstream_telemetry: Option<&ExecutionTelemetry>,
) -> ExecutionTelemetry {
    let elapsed_ms = stream_elapsed_ms_at(stream_started_at, event_observed_at);
    ExecutionTelemetry {
        ttfb_ms: Some(elapsed_ms),
        elapsed_ms: Some(elapsed_ms),
        upstream_bytes: upstream_telemetry.and_then(|telemetry| telemetry.upstream_bytes),
    }
}

fn maybe_capture_first_stream_event_telemetry(
    stream_started_at: Instant,
    event_observed_at: Instant,
    upstream_telemetry: Option<&ExecutionTelemetry>,
    usage_stream_telemetry: &mut Option<ExecutionTelemetry>,
) -> bool {
    if usage_stream_telemetry
        .as_ref()
        .and_then(|telemetry| telemetry.ttfb_ms)
        .is_some()
    {
        return false;
    }

    *usage_stream_telemetry = Some(first_stream_event_telemetry(
        stream_started_at,
        event_observed_at,
        upstream_telemetry,
    ));
    true
}

fn usage_refresh_telemetry(
    upstream_telemetry: &ExecutionTelemetry,
    usage_stream_telemetry: Option<&ExecutionTelemetry>,
) -> ExecutionTelemetry {
    ExecutionTelemetry {
        ttfb_ms: usage_stream_telemetry.and_then(|telemetry| telemetry.ttfb_ms),
        elapsed_ms: upstream_telemetry.elapsed_ms,
        upstream_bytes: upstream_telemetry.upstream_bytes,
    }
}

fn maybe_record_first_stream_event_started(
    state: &AppState,
    lifecycle_seed: &LifecycleUsageSeed,
    status_code: u16,
    stream_started_at: Instant,
    event_observed_at: Instant,
    upstream_telemetry: Option<&ExecutionTelemetry>,
    usage_stream_telemetry: &mut Option<ExecutionTelemetry>,
) {
    if !maybe_capture_first_stream_event_telemetry(
        stream_started_at,
        event_observed_at,
        upstream_telemetry,
        usage_stream_telemetry,
    ) {
        return;
    }
    let Some(telemetry) = usage_stream_telemetry.as_ref() else {
        return;
    };
    state.usage_runtime.record_stream_started(
        state.usage_lifecycle_data_state().as_ref(),
        lifecycle_seed,
        status_code,
        Some(telemetry),
    );
}

fn build_terminal_stream_telemetry(
    stream_started_at: Instant,
    telemetry: Option<&ExecutionTelemetry>,
    usage_stream_telemetry: Option<&ExecutionTelemetry>,
    upstream_bytes: u64,
) -> ExecutionTelemetry {
    let current_elapsed_ms = stream_elapsed_ms_since(stream_started_at);
    let ttfb_ms = usage_stream_telemetry.and_then(|telemetry| telemetry.ttfb_ms);
    let prior_elapsed_ms = telemetry
        .and_then(|telemetry| telemetry.elapsed_ms)
        .or_else(|| usage_stream_telemetry.and_then(|telemetry| telemetry.elapsed_ms))
        .unwrap_or(0);
    let elapsed_ms = current_elapsed_ms
        .max(prior_elapsed_ms)
        .max(ttfb_ms.unwrap_or(0));
    ExecutionTelemetry {
        ttfb_ms,
        elapsed_ms: Some(elapsed_ms),
        upstream_bytes: Some(upstream_bytes),
    }
}

fn should_skip_direct_finalize_prefetch(
    direct_stream_finalize_kind: Option<&str>,
    content_type: Option<&str>,
    provider_api_format: &str,
    client_api_format: &str,
    has_private_stream_normalizer: bool,
    has_local_stream_rewriter: bool,
    force_prefetch: bool,
) -> bool {
    StreamCommitPolicy::for_response(
        direct_stream_finalize_kind.is_some(),
        content_type,
        provider_api_format,
        client_api_format,
        has_private_stream_normalizer,
        has_local_stream_rewriter,
        force_prefetch,
    )
    .commits_on_response_headers()
}

fn prefetched_openai_responses_body_has_output_boundary(body: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(body) else {
        return true;
    };
    for line in text.lines() {
        let Some(data) = line.trim().strip_prefix("data:").map(str::trim) else {
            continue;
        };
        if data.is_empty() {
            continue;
        }
        if data == "[DONE]" {
            return true;
        }
        let Ok(event) = serde_json::from_str::<Value>(data) else {
            continue;
        };
        let event_type = event.get("type").and_then(Value::as_str).map(str::trim);
        if !event_type.is_some_and(|event_type| {
            matches!(
                event_type,
                "response.created" | "response.in_progress" | "response.queued"
            )
        }) {
            return true;
        }
    }
    false
}

fn should_probe_success_failover_before_stream(headers: &BTreeMap<String, String>) -> bool {
    let content_type = headers
        .get("content-type")
        .map(String::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
        .to_ascii_lowercase();

    content_type.contains("json") || content_type.ends_with("+json")
}

async fn probe_local_stream_success_failover_text<R>(
    buffered_frames: &mut VecDeque<ObservedStreamFrame>,
    lines: &mut FramedRead<R, LinesCodec>,
) -> Result<Option<String>, GatewayError>
where
    R: tokio::io::AsyncRead + Unpin,
{
    while let Some(observed_frame) = read_next_observed_stream_frame(lines).await? {
        let probe_text = match &observed_frame.frame.payload {
            StreamFramePayload::Data { chunk_b64, text } => {
                match decode_stream_data_chunk(chunk_b64.as_deref(), text.as_deref()) {
                    Ok(chunk) if !chunk.is_empty() => {
                        Some(String::from_utf8_lossy(&chunk).into_owned())
                    }
                    Ok(_) | Err(_) => None,
                }
            }
            StreamFramePayload::Error { .. } | StreamFramePayload::Eof { .. } => None,
            StreamFramePayload::Headers { .. } | StreamFramePayload::Telemetry { .. } => None,
        };
        buffered_frames.push_back(observed_frame);
        if probe_text.is_some() {
            return Ok(probe_text);
        }
    }

    Ok(None)
}

#[allow(clippy::too_many_arguments)]
fn apply_stream_summary_report_context(
    execution: &mut DirectUpstreamStreamExecution,
    report_context: Option<&Value>,
) {
    if let Some(report_context) = report_context.cloned() {
        execution.stream_summary_report_context = report_context;
    }
}
