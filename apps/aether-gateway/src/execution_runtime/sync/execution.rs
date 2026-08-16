use std::collections::BTreeMap;
use std::io::Error as IoError;
use std::sync::Arc;
use std::time::{Duration, Instant};

use aether_ai_serving::{AiAttemptExecutionOutcome, AiAttemptRetryScope, UPSTREAM_IS_STREAM_KEY};
use aether_contracts::{
    ExecutionPlan, ExecutionResponseObservation, ExecutionResult, ExecutionTelemetry,
};
use aether_data_contracts::repository::candidates::RequestCandidateStatus;
use aether_scheduler_core::{
    execution_error_details, parse_request_candidate_report_context,
    SchedulerRequestCandidateStatusUpdate,
};
use aether_usage_runtime::{
    build_lifecycle_usage_seed, build_sync_terminal_usage_payload_seed,
    build_terminal_usage_context_seed, build_usage_event_data_seed, UsageEvent, UsageEventType,
};
use async_stream::stream;
use axum::body::{to_bytes, Body, Bytes};
use axum::http::header::{CACHE_CONTROL, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE};
use axum::http::{HeaderName, HeaderValue, Response, StatusCode};
use futures_util::StreamExt;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tokio::time::MissedTickBehavior;
use tracing::{debug, warn};

use crate::api::response::{
    attach_control_metadata_headers, build_client_response, build_client_response_from_parts,
    build_client_response_from_parts_with_mutator,
};
use crate::clock::current_unix_ms as current_request_candidate_unix_ms;
use crate::control::GatewayControlDecision;
#[cfg(test)]
use crate::execution_runtime::remote_test_support::post_sync_plan_to_remote_execution_runtime;
use crate::execution_runtime::submission::submit_local_core_error_or_sync_finalize;
use crate::execution_runtime::transport::{
    append_upstream_response_body_chunk, build_execution_response_body, build_request_body,
    collect_response_headers, decode_response_body_bytes, execution_response_body_mode,
    format_hyper_error_chain, format_upstream_request_error, format_wreq_upstream_request_error,
    response_body_is_json, send_request, DirectHttpResponse, DirectSyncExecutionRuntime,
    ExecutionRuntimeTransportError,
};
use crate::execution_runtime::{
    ai_attempt_retry_scope_from_failure_disposition, analyze_local_candidate_failover_sync,
    apply_endpoint_response_header_rules, attach_provider_response_headers_to_report_context,
    local_failover_response_text, should_fallback_to_control_sync, should_finalize_sync_response,
    LocalFailoverDecision,
};
use crate::orchestration::{
    apply_local_execution_effect, build_local_error_flow_metadata, trace_upstream_response_body,
    with_error_flow_report_context, with_upstream_response_report_context,
    LocalAdaptiveRateLimitEffect, LocalAdaptiveSuccessEffect, LocalAttemptFailureEffect,
    LocalExecutionEffect, LocalExecutionEffectContext, LocalHealthFailureEffect,
    LocalHealthSuccessEffect,
};
use crate::request_candidate_runtime::{
    ensure_execution_request_candidate_slot, record_local_request_candidate_extra_data,
    record_local_request_candidate_status, record_local_request_candidate_status_snapshot,
    snapshot_local_request_candidate_status,
};
use crate::request_diagnostics::{
    attach_current_request_diagnostics_and_candidate_start_timing_to_report_context,
    attach_request_diagnostics_to_report_context, calibrate_candidate_first_byte_elapsed_ms,
    current_request_diagnostics, RequestDiagnostics,
};
use crate::usage::{spawn_sync_report, submit_sync_report};
use crate::{usage::GatewaySyncReportRequest, AppState, GatewayError};
use aether_gateway_frontdoor::short_request_id;

#[path = "execution/policy.rs"]
mod policy;
use policy::decode_execution_result_body;

const OPENAI_IMAGE_SYNC_PLAN_KIND: &str = "openai_image_sync";
const OPENAI_IMAGE_SYNC_DEFAULT_TOTAL_TIMEOUT_MS: u64 = 900_000;
const SYNC_EXECUTION_IDLE_LOG_INTERVAL: Duration = Duration::from_secs(60);
const OPENAI_IMAGE_SYNC_JSON_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);
const OPENAI_IMAGE_SYNC_JSON_HEARTBEAT_BYTES: &[u8] = b"\n";
const OPENAI_IMAGE_SYNC_PROGRESS_WRITE_INTERVAL: Duration = Duration::from_secs(5);

fn elapsed_ms_since(started_at: Instant) -> u64 {
    started_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}

fn calibrated_sync_candidate_first_byte_elapsed_ms(
    candidate_started_at: Instant,
    result: &ExecutionResult,
) -> Option<u64> {
    let telemetry = result.telemetry.as_ref()?;
    calibrate_candidate_first_byte_elapsed_ms(
        elapsed_ms_since(candidate_started_at),
        telemetry.elapsed_ms,
        telemetry.ttfb_ms,
    )
}

#[derive(Debug)]
struct SyncExecutionFailure {
    error_type: &'static str,
    message: String,
    status_code: Option<u16>,
    latency_ms: Option<u64>,
    fallback_kind: Option<SyncExecutionFailureFallbackKind>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SyncExecutionFailureFallbackKind {
    UpstreamResponseTooLarge,
    UpstreamResponseDecode,
}

impl SyncExecutionFailureFallbackKind {
    fn error_type(self) -> &'static str {
        match self {
            Self::UpstreamResponseTooLarge => "upstream_response_too_large",
            Self::UpstreamResponseDecode => "upstream_response_decode_failed",
        }
    }

    fn client_message(self) -> &'static str {
        match self {
            Self::UpstreamResponseTooLarge => "Upstream response too large",
            Self::UpstreamResponseDecode => "Failed to decode upstream response",
        }
    }
}

struct SyncAttemptTerminalGuard {
    state: AppState,
    plan: ExecutionPlan,
    report_context: Option<Value>,
    request_diagnostics: Option<Arc<RequestDiagnostics>>,
    candidate_started_unix_ms: u64,
    candidate_started_at: Instant,
    armed: bool,
}

impl SyncAttemptTerminalGuard {
    fn new(
        state: &AppState,
        plan: &ExecutionPlan,
        report_context: Option<Value>,
        candidate_started_unix_ms: u64,
        candidate_started_at: Instant,
    ) -> Self {
        Self {
            state: state.clone(),
            plan: plan.clone(),
            report_context,
            request_diagnostics: current_request_diagnostics(),
            candidate_started_unix_ms,
            candidate_started_at,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }

    async fn fail_and_disarm(&mut self, error: &GatewayError) {
        if !self.armed {
            return;
        }
        self.armed = false;
        record_sync_attempt_forced_terminal_state(
            self.state.clone(),
            self.plan.clone(),
            self.report_context.clone(),
            self.request_diagnostics.clone(),
            self.candidate_started_unix_ms,
            self.candidate_started_at,
            UsageEventType::Failed,
            RequestCandidateStatus::Failed,
            StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
            "local_sync_attempt_aborted",
            format!("Local sync attempt failed before terminal finalization: {error:?}"),
        )
        .await;
    }
}

impl Drop for SyncAttemptTerminalGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        self.armed = false;
        let state = self.state.clone();
        let plan = self.plan.clone();
        let report_context = self.report_context.clone();
        let request_diagnostics = self.request_diagnostics.clone();
        let candidate_started_unix_ms = self.candidate_started_unix_ms;
        let candidate_started_at = self.candidate_started_at;
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                record_sync_attempt_forced_terminal_state(
                    state,
                    plan,
                    report_context,
                    request_diagnostics,
                    candidate_started_unix_ms,
                    candidate_started_at,
                    UsageEventType::Cancelled,
                    RequestCandidateStatus::Cancelled,
                    499,
                    "local_sync_attempt_cancelled",
                    "Local sync attempt was dropped before terminal finalization, usually because the client disconnected or the request task was cancelled.",
                )
                .await;
            });
        } else {
            warn!(
                event_name = "local_sync_attempt_terminal_guard_no_runtime",
                log_type = "ops",
                request_id = %short_request_id(self.plan.request_id.as_str()),
                candidate_id = ?self.plan.candidate_id,
                "gateway could not finalize dropped local sync attempt because no Tokio runtime is available"
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn record_sync_attempt_forced_terminal_state(
    state: AppState,
    plan: ExecutionPlan,
    report_context: Option<Value>,
    request_diagnostics: Option<Arc<RequestDiagnostics>>,
    candidate_started_unix_ms: u64,
    candidate_started_at: Instant,
    usage_event_type: UsageEventType,
    candidate_status: RequestCandidateStatus,
    status_code: u16,
    error_type: &'static str,
    error_message: impl Into<String>,
) {
    let error_message = error_message.into();
    let report_context =
        attach_request_diagnostics_to_report_context(report_context, request_diagnostics.as_ref());
    let terminal_unix_ms = current_request_candidate_unix_ms();
    let latency_ms = elapsed_ms_since(candidate_started_at);
    record_local_request_candidate_status(
        &state,
        &plan,
        report_context.as_ref(),
        SchedulerRequestCandidateStatusUpdate {
            status: candidate_status,
            status_code: Some(status_code),
            error_type: Some(error_type.to_string()),
            error_message: Some(error_message.clone()),
            latency_ms: Some(latency_ms),
            started_at_unix_ms: Some(candidate_started_unix_ms),
            finished_at_unix_ms: Some(terminal_unix_ms),
        },
    )
    .await;

    if !state.usage_runtime.is_enabled() {
        return;
    }

    let mut usage_data = build_usage_event_data_seed(&plan, report_context.as_ref());
    usage_data.status_code = Some(status_code);
    usage_data.error_message = Some(error_message.clone());
    usage_data.error_category = Some(
        match usage_event_type {
            UsageEventType::Cancelled => "cancelled",
            _ => "server_error",
        }
        .to_string(),
    );
    usage_data.response_time_ms = Some(latency_ms);
    let error_body = json!({
        "error": {
            "type": error_type,
            "message": error_message,
            "code": status_code
        }
    });
    usage_data.response_headers = Some(json!({"content-type": "application/json"}));
    usage_data.response_body = Some(error_body.clone());
    usage_data.client_response_headers = Some(json!({"content-type": "application/json"}));
    usage_data.client_response_body = Some(error_body);

    state
        .usage_runtime
        .record_terminal_event_direct(
            state.usage_lifecycle_data_state().as_ref(),
            UsageEvent::new(usage_event_type, plan.request_id.clone(), usage_data),
        )
        .await;
}

impl SyncExecutionFailure {
    fn from_transport(err: ExecutionRuntimeTransportError) -> Self {
        let fallback_kind = match &err {
            ExecutionRuntimeTransportError::UpstreamResponseTooLarge { .. } => {
                Some(SyncExecutionFailureFallbackKind::UpstreamResponseTooLarge)
            }
            ExecutionRuntimeTransportError::UpstreamResponseDecode { .. } => {
                Some(SyncExecutionFailureFallbackKind::UpstreamResponseDecode)
            }
            _ => None,
        };
        Self {
            error_type: fallback_kind
                .map(SyncExecutionFailureFallbackKind::error_type)
                .unwrap_or("execution_runtime_unavailable"),
            message: err.to_string(),
            status_code: fallback_kind.map(|_| StatusCode::BAD_GATEWAY.as_u16()),
            latency_ms: None,
            fallback_kind,
        }
    }

    fn image_sync_total_timeout(timeout_ms: u64, elapsed_ms: u64) -> Self {
        Self {
            error_type: "image_sync_total_timeout",
            message: format!(
                "OpenAI image sync execution exceeded total timeout of {timeout_ms}ms"
            ),
            status_code: Some(StatusCode::GATEWAY_TIMEOUT.as_u16()),
            latency_ms: Some(elapsed_ms),
            fallback_kind: None,
        }
    }
}

fn build_sync_execution_failure_fallback_body(
    client_api_format: &str,
    kind: SyncExecutionFailureFallbackKind,
) -> Value {
    let message = kind.client_message();
    let error_type = kind.error_type();
    match crate::ai_serving::normalize_api_format_alias(client_api_format).as_str() {
        "claude:messages" => json!({
            "type": "error",
            "error": {
                "type": "upstream_error",
                "message": message,
            }
        }),
        "gemini:generate_content" => json!({
            "error": {
                "code": StatusCode::BAD_GATEWAY.as_u16(),
                "message": message,
                "status": "BAD_GATEWAY",
            }
        }),
        _ => json!({
            "error": {
                "type": "upstream_error",
                "message": message,
                "code": error_type,
            }
        }),
    }
}

fn build_sync_execution_failure_fallback_response(
    failure: &SyncExecutionFailure,
    plan: &ExecutionPlan,
    trace_id: &str,
    decision: &GatewayControlDecision,
) -> Result<Option<Response<Body>>, GatewayError> {
    let Some(kind) = failure.fallback_kind else {
        return Ok(None);
    };
    let body_json = build_sync_execution_failure_fallback_body(&plan.client_api_format, kind);
    let body_bytes = serde_json::to_vec(&body_json)
        .map_err(|error| GatewayError::Internal(error.to_string()))?;
    let headers = BTreeMap::from([
        ("content-type".to_string(), "application/json".to_string()),
        ("content-length".to_string(), body_bytes.len().to_string()),
    ]);
    let response = build_client_response_from_parts(
        StatusCode::BAD_GATEWAY.as_u16(),
        &headers,
        Body::from(body_bytes),
        trace_id,
        Some(decision),
    )?;
    attach_control_metadata_headers(
        response,
        Some(plan.request_id.as_str()),
        plan.candidate_id.as_deref(),
    )
    .map(Some)
}

fn maybe_store_sync_execution_failure_fallback(
    failure: &SyncExecutionFailure,
    plan: &ExecutionPlan,
    trace_id: &str,
    decision: &GatewayControlDecision,
    retry_scope_out: &mut Option<&mut AiAttemptRetryScope>,
    retry_fallback_out: &mut Option<&mut Option<Response<Body>>>,
) -> Result<(), GatewayError> {
    if failure.fallback_kind.is_none() {
        return Ok(());
    }
    if let Some(retry_scope) = retry_scope_out.as_deref_mut() {
        *retry_scope = AiAttemptRetryScope::Candidate;
    }
    if let Some(retry_fallback) = retry_fallback_out.as_deref_mut() {
        *retry_fallback =
            build_sync_execution_failure_fallback_response(failure, plan, trace_id, decision)?;
    }
    Ok(())
}

async fn maybe_build_sync_transport_error_stop_response(
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
        StatusCode::BAD_GATEWAY.as_u16(),
        error_type,
        error_message,
        elapsed_ms,
    )
    .await
    .map(Some)
}

fn spawn_sync_candidate_status_update(
    state: AppState,
    snapshot: crate::request_candidate_runtime::LocalRequestCandidateStatusSnapshot,
    status_update: SchedulerRequestCandidateStatusUpdate,
) {
    tokio::spawn(async move {
        record_local_request_candidate_status_snapshot(&state, &snapshot, status_update).await;
    });
}

fn record_sync_response_started(
    state: &AppState,
    lifecycle_seed: aether_usage_runtime::LifecycleUsageSeed,
    request_candidate_status_snapshot: Option<
        crate::request_candidate_runtime::LocalRequestCandidateStatusSnapshot,
    >,
    candidate_started_unix_ms: u64,
    status_code: u16,
    ttfb_ms: u64,
) {
    state.usage_runtime.record_stream_started_immediate_async(
        state.usage_lifecycle_data_state().as_ref(),
        lifecycle_seed,
        status_code,
        Some(ExecutionTelemetry {
            ttfb_ms: Some(ttfb_ms),
            elapsed_ms: Some(ttfb_ms),
            upstream_bytes: None,
        }),
    );

    if let Some(snapshot) = request_candidate_status_snapshot {
        spawn_sync_candidate_status_update(
            state.clone(),
            snapshot,
            SchedulerRequestCandidateStatusUpdate {
                status: RequestCandidateStatus::Streaming,
                status_code: Some(status_code),
                error_type: None,
                error_message: None,
                latency_ms: Some(ttfb_ms),
                started_at_unix_ms: Some(candidate_started_unix_ms),
                finished_at_unix_ms: None,
            },
        );
    }
}

fn record_sync_execution_active(
    state: &AppState,
    plan: &ExecutionPlan,
    report_context: Option<&Value>,
    candidate_started_unix_ms: u64,
) {
    let lifecycle_seed = build_lifecycle_usage_seed(plan, report_context);
    state.usage_runtime.record_sync_active_immediate_async(
        state.usage_lifecycle_data_state().as_ref(),
        lifecycle_seed,
    );

    if let Some(snapshot) = snapshot_local_request_candidate_status(plan, report_context) {
        spawn_sync_candidate_status_update(
            state.clone(),
            snapshot,
            SchedulerRequestCandidateStatusUpdate {
                status: RequestCandidateStatus::Streaming,
                status_code: None,
                error_type: None,
                error_message: None,
                latency_ms: None,
                started_at_unix_ms: Some(candidate_started_unix_ms),
                finished_at_unix_ms: None,
            },
        );
    }
}

async fn record_sync_terminal_usage(
    state: &AppState,
    plan: &ExecutionPlan,
    report_context: Option<&serde_json::Value>,
    payload: &GatewaySyncReportRequest,
    candidate_started_at: Instant,
    candidate_first_byte_elapsed_ms: Option<u64>,
) {
    let report_context_with_diagnostics =
        attach_current_request_diagnostics_and_candidate_start_timing_to_report_context(
            report_context,
            candidate_started_at,
            candidate_first_byte_elapsed_ms,
        );
    let context_seed = build_terminal_usage_context_seed(
        plan,
        report_context_with_diagnostics.as_ref().or(report_context),
    );
    let payload_seed = build_sync_terminal_usage_payload_seed(payload);
    state
        .usage_runtime
        .record_sync_terminal(
            state.usage_lifecycle_data_state().as_ref(),
            context_seed,
            payload_seed,
        )
        .await;
}

async fn record_sync_terminal_usage_and_disarm_guard(
    state: &AppState,
    plan: &ExecutionPlan,
    report_context: Option<&serde_json::Value>,
    payload: &GatewaySyncReportRequest,
    candidate_started_at: Instant,
    candidate_first_byte_elapsed_ms: Option<u64>,
    terminal_guard: &mut SyncAttemptTerminalGuard,
) {
    record_sync_terminal_usage(
        state,
        plan,
        report_context,
        payload,
        candidate_started_at,
        candidate_first_byte_elapsed_ms,
    )
    .await;
    terminal_guard.disarm();
}

fn with_sync_error_trace_context(
    report_context: Option<&serde_json::Value>,
    status_code: u16,
    headers: &BTreeMap<String, String>,
    body_json: Option<&serde_json::Value>,
    body_bytes: &[u8],
    response_text: Option<&str>,
    local_failover_analysis: crate::orchestration::LocalFailoverAnalysis,
) -> Option<serde_json::Value> {
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

fn build_sync_report_payload(
    trace_id: &str,
    report_kind: String,
    report_context: Option<serde_json::Value>,
    status_code: u16,
    headers: BTreeMap<String, String>,
    body_json: Option<serde_json::Value>,
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

#[derive(Debug, Clone)]
struct OpenAiImageSyncProgressSnapshot {
    phase: &'static str,
    upstream_ttfb_ms: Option<u64>,
    upstream_sse_frame_count: u64,
    last_upstream_event: Option<String>,
    last_upstream_frame_at_unix_ms: Option<u64>,
    partial_image_count: u64,
    last_client_visible_event: Option<String>,
    downstream_heartbeat_count: u64,
    last_downstream_heartbeat_at_unix_ms: Option<u64>,
    downstream_heartbeat_interval_ms: Option<u64>,
}

struct OpenAiImageSyncProgressRecorder<'a> {
    state: &'a AppState,
    plan: &'a ExecutionPlan,
    report_context: Option<&'a Value>,
    snapshot: Arc<Mutex<OpenAiImageSyncProgressSnapshot>>,
    buffer: Vec<u8>,
    last_persist_at: Option<Instant>,
}

#[derive(Clone)]
struct OpenAiImageSyncJsonHeartbeatContext {
    state: AppState,
    plan: ExecutionPlan,
    report_context: Option<Value>,
    snapshot: Arc<Mutex<OpenAiImageSyncProgressSnapshot>>,
    started_at: Instant,
    trace_id: String,
    request_id_for_log: String,
    candidate_id: Option<String>,
}

#[derive(Debug)]
struct OpenAiImageSyncSseFrame {
    event_name: String,
    is_partial_image: bool,
    is_completed: bool,
    is_failed: bool,
    client_visible_event: Option<&'static str>,
}

impl OpenAiImageSyncProgressSnapshot {
    fn new() -> Self {
        Self {
            phase: "upstream_connecting",
            upstream_ttfb_ms: None,
            upstream_sse_frame_count: 0,
            last_upstream_event: None,
            last_upstream_frame_at_unix_ms: None,
            partial_image_count: 0,
            last_client_visible_event: None,
            downstream_heartbeat_count: 0,
            last_downstream_heartbeat_at_unix_ms: None,
            downstream_heartbeat_interval_ms: None,
        }
    }

    fn to_json(&self) -> Value {
        json!({
            "phase": self.phase,
            "upstream_ttfb_ms": self.upstream_ttfb_ms,
            "upstream_sse_frame_count": self.upstream_sse_frame_count,
            "last_upstream_event": self.last_upstream_event,
            "last_upstream_frame_at_unix_ms": self.last_upstream_frame_at_unix_ms,
            "partial_image_count": self.partial_image_count,
            "last_client_visible_event": self.last_client_visible_event,
            "downstream_heartbeat_count": self.downstream_heartbeat_count,
            "last_downstream_heartbeat_at_unix_ms": self.last_downstream_heartbeat_at_unix_ms,
            "downstream_heartbeat_interval_ms": self.downstream_heartbeat_interval_ms,
        })
    }
}

impl<'a> OpenAiImageSyncProgressRecorder<'a> {
    fn new(
        state: &'a AppState,
        plan: &'a ExecutionPlan,
        report_context: Option<&'a Value>,
        snapshot: Option<Arc<Mutex<OpenAiImageSyncProgressSnapshot>>>,
    ) -> Self {
        Self {
            state,
            plan,
            report_context,
            snapshot: snapshot
                .unwrap_or_else(|| Arc::new(Mutex::new(OpenAiImageSyncProgressSnapshot::new()))),
            buffer: Vec::new(),
            last_persist_at: None,
        }
    }

    async fn persist(
        &mut self,
        status: RequestCandidateStatus,
        status_code: Option<u16>,
        latency_ms: Option<u64>,
        force: bool,
    ) {
        let now = Instant::now();
        if !force
            && self.last_persist_at.is_some_and(|last| {
                now.duration_since(last) < OPENAI_IMAGE_SYNC_PROGRESS_WRITE_INTERVAL
            })
        {
            return;
        }
        let snapshot = self.snapshot.lock().await.clone();
        let extra_data = json!({
            "image_progress": snapshot.to_json(),
        });
        record_local_request_candidate_extra_data(
            self.state,
            self.plan,
            self.report_context,
            status,
            status_code,
            latency_ms,
            extra_data,
        )
        .await;
        self.last_persist_at = Some(now);
    }

    async fn record_connecting(&mut self) {
        self.snapshot.lock().await.phase = "upstream_connecting";
        self.persist(RequestCandidateStatus::Pending, None, None, true)
            .await;
    }

    async fn record_response_started(&mut self, status_code: u16, ttfb_ms: u64) {
        {
            let mut snapshot = self.snapshot.lock().await;
            snapshot.phase = if status_code >= 400 {
                "failed"
            } else {
                "upstream_streaming"
            };
            snapshot.upstream_ttfb_ms = Some(ttfb_ms);
        }
        self.persist(
            if status_code >= 400 {
                RequestCandidateStatus::Failed
            } else {
                RequestCandidateStatus::Streaming
            },
            Some(status_code),
            Some(ttfb_ms),
            true,
        )
        .await;
    }

    async fn observe_chunk(&mut self, chunk: &[u8], status_code: u16, elapsed_ms: u64) {
        if chunk.is_empty() {
            return;
        }
        self.buffer.extend_from_slice(chunk);
        let mut force_persist = false;
        while let Some(block_end) = find_sse_block_end(&self.buffer) {
            let block = self.buffer.drain(..block_end).collect::<Vec<_>>();
            let Some(frame) = parse_openai_image_sync_sse_frame(&block) else {
                continue;
            };
            {
                let mut snapshot = self.snapshot.lock().await;
                snapshot.upstream_sse_frame_count =
                    snapshot.upstream_sse_frame_count.saturating_add(1);
                snapshot.last_upstream_event = Some(frame.event_name);
                snapshot.last_upstream_frame_at_unix_ms = Some(current_request_candidate_unix_ms());
                if frame.is_partial_image {
                    snapshot.partial_image_count = snapshot.partial_image_count.saturating_add(1);
                }
                if let Some(client_visible_event) = frame.client_visible_event {
                    snapshot.last_client_visible_event = Some(client_visible_event.to_string());
                    force_persist = true;
                }
                if frame.is_failed || status_code >= 400 {
                    snapshot.phase = "failed";
                    force_persist = true;
                } else if frame.is_completed {
                    snapshot.phase = "upstream_completed";
                    force_persist = true;
                } else {
                    snapshot.phase = "upstream_streaming";
                }
            }
        }
        let phase = self.snapshot.lock().await.phase;
        self.persist(
            if phase == "failed" {
                RequestCandidateStatus::Failed
            } else {
                RequestCandidateStatus::Streaming
            },
            Some(status_code),
            Some(elapsed_ms),
            force_persist,
        )
        .await;
    }

    async fn finish(&mut self, status_code: u16, elapsed_ms: u64) {
        {
            let mut snapshot = self.snapshot.lock().await;
            if status_code >= 400 || snapshot.phase == "failed" {
                snapshot.phase = "failed";
            } else {
                snapshot.phase = "upstream_completed";
            }
        }
        self.persist(
            if status_code >= 400 {
                RequestCandidateStatus::Failed
            } else {
                RequestCandidateStatus::Streaming
            },
            Some(status_code),
            Some(elapsed_ms),
            true,
        )
        .await;
    }

    async fn fail(&mut self, status_code: Option<u16>, elapsed_ms: u64) {
        self.snapshot.lock().await.phase = "failed";
        self.persist(
            RequestCandidateStatus::Failed,
            status_code,
            Some(elapsed_ms),
            true,
        )
        .await;
    }
}

impl OpenAiImageSyncJsonHeartbeatContext {
    async fn record_heartbeat(&self, heartbeat_kind: &'static str, heartbeat_interval: Duration) {
        let now_unix_ms = current_request_candidate_unix_ms();
        let elapsed_ms = self.started_at.elapsed().as_millis() as u64;
        let interval_ms = heartbeat_interval
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX);
        let (count, phase, progress_json) = {
            let mut snapshot = self.snapshot.lock().await;
            snapshot.downstream_heartbeat_count =
                snapshot.downstream_heartbeat_count.saturating_add(1);
            snapshot.last_downstream_heartbeat_at_unix_ms = Some(now_unix_ms);
            snapshot.downstream_heartbeat_interval_ms = Some(interval_ms);
            (
                snapshot.downstream_heartbeat_count,
                snapshot.phase,
                snapshot.to_json(),
            )
        };
        let status = match phase {
            "failed" => RequestCandidateStatus::Failed,
            "upstream_connecting" => RequestCandidateStatus::Pending,
            _ => RequestCandidateStatus::Streaming,
        };
        record_local_request_candidate_extra_data(
            &self.state,
            &self.plan,
            self.report_context.as_ref(),
            status,
            None,
            Some(elapsed_ms),
            json!({ "image_progress": progress_json }),
        )
        .await;
        debug!(
            event_name = "openai_image_sync_json_heartbeat_sent",
            log_type = "event",
            trace_id = %self.trace_id,
            request_id = %self.request_id_for_log,
            candidate_id = self.candidate_id.as_deref().unwrap_or("-"),
            heartbeat_kind,
            heartbeat_count = count,
            heartbeat_interval_ms = interval_ms,
            elapsed_ms,
            phase,
            "gateway emitted OpenAI image sync JSON whitespace heartbeat"
        );
    }
}

fn find_sse_block_end(buffer: &[u8]) -> Option<usize> {
    buffer
        .windows(2)
        .position(|window| window == b"\n\n")
        .map(|index| index + 2)
        .or_else(|| {
            buffer
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .map(|index| index + 4)
        })
}

fn parse_openai_image_sync_sse_frame(block: &[u8]) -> Option<OpenAiImageSyncSseFrame> {
    let text = std::str::from_utf8(block).ok()?.trim();
    if text.is_empty() {
        return None;
    }

    let mut event_name = None;
    let mut data_lines = Vec::new();
    for line in text.lines() {
        let line = line.trim_end_matches('\r').trim();
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        if let Some(value) = line.strip_prefix("event:") {
            event_name = Some(value.trim().to_string());
        } else if let Some(value) = line.strip_prefix("data:") {
            data_lines.push(value.trim().to_string());
        }
    }

    let data_text = data_lines.join("\n");
    if data_text.trim().eq("[DONE]") {
        let event_name = event_name.unwrap_or_else(|| "done".to_string());
        return Some(OpenAiImageSyncSseFrame {
            event_name,
            is_partial_image: false,
            is_completed: true,
            is_failed: false,
            client_visible_event: None,
        });
    }

    let data_event_name = serde_json::from_str::<Value>(&data_text)
        .ok()
        .and_then(|value| {
            value
                .get("type")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| {
                    value
                        .get("error")
                        .and_then(Value::as_object)
                        .map(|_| "error".to_string())
                })
        });
    let event_name = event_name.or(data_event_name)?;
    let is_partial_image = event_name == "response.image_generation_call.partial_image";
    let is_completed = event_name == "response.completed";
    let is_failed = event_name == "response.failed"
        || event_name == "response.error"
        || event_name == "error"
        || event_name.ends_with(".failed");
    let client_visible_event = if is_partial_image {
        Some("image_generation.partial_image")
    } else if is_completed {
        Some("image_generation.completed")
    } else if is_failed {
        Some("image_generation.failed")
    } else {
        None
    };

    Some(OpenAiImageSyncSseFrame {
        event_name,
        is_partial_image,
        is_completed,
        is_failed,
        client_visible_event,
    })
}

#[allow(clippy::too_many_arguments)]
async fn execute_direct_sync_runtime_candidate(
    state: &AppState,
    plan: &ExecutionPlan,
    report_context: Option<&Value>,
    trace_id: &str,
    plan_kind: &str,
    candidate_started_unix_ms: u64,
    request_id_for_log: &str,
    candidate_id: Option<&str>,
    provider_name: &str,
    endpoint_id: &str,
    key_id: &str,
    model_name: &str,
    candidate_index: &str,
    progress_snapshot: Option<Arc<Mutex<OpenAiImageSyncProgressSnapshot>>>,
) -> Result<ExecutionResult, SyncExecutionFailure> {
    if !should_track_openai_image_sync_upstream_sse(plan_kind, plan, report_context) {
        let state_for_response_started = state.clone();
        let response_started_lifecycle_seed = build_lifecycle_usage_seed(plan, report_context);
        let response_started_candidate_snapshot =
            snapshot_local_request_candidate_status(plan, report_context);
        return DirectSyncExecutionRuntime::new()
            .execute_sync_with_response_started(plan, move |event| {
                record_sync_response_started(
                    &state_for_response_started,
                    response_started_lifecycle_seed,
                    response_started_candidate_snapshot,
                    candidate_started_unix_ms,
                    event.status_code,
                    event.ttfb_ms,
                );
            })
            .await
            .map_err(SyncExecutionFailure::from_transport);
    }

    let started_at = Instant::now();
    let timeout_ms = resolve_openai_image_sync_total_timeout_ms(plan);
    let mut execution = Box::pin(execute_openai_image_sync_upstream_sse_candidate(
        state,
        plan,
        report_context,
        progress_snapshot.clone(),
    ));
    let mut idle_interval = tokio::time::interval(SYNC_EXECUTION_IDLE_LOG_INTERVAL);
    idle_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    idle_interval.tick().await;
    let mut total_timeout = Box::pin(tokio::time::sleep(Duration::from_millis(timeout_ms)));

    loop {
        tokio::select! {
            result = execution.as_mut() => {
                match result {
                    Ok(result) => return Ok(result),
                    Err(err) => {
                        let elapsed_ms = started_at.elapsed().as_millis() as u64;
                        let status_code = err.status_code;
                        record_openai_image_sync_failed_progress(
                            state,
                            plan,
                            report_context,
                            status_code,
                            elapsed_ms,
                            progress_snapshot.clone(),
                        )
                        .await;
                        return Err(err);
                    }
                }
            }
            _ = idle_interval.tick() => {
                warn!(
                    event_name = "openai_image_sync_execution_idle",
                    log_type = "ops",
                    trace_id = %trace_id,
                    request_id = %request_id_for_log,
                    candidate_id = candidate_id.unwrap_or("-"),
                    provider_name,
                    endpoint_id,
                    key_id,
                    model_name,
                    candidate_index,
                    elapsed_ms = started_at.elapsed().as_millis() as u64,
                    timeout_ms,
                    "gateway OpenAI image sync execution still waiting for upstream response"
                );
            }
            _ = total_timeout.as_mut() => {
                let elapsed_ms = started_at.elapsed().as_millis() as u64;
                record_openai_image_sync_failed_progress(
                    state,
                    plan,
                    report_context,
                    Some(StatusCode::GATEWAY_TIMEOUT.as_u16()),
                    elapsed_ms,
                    progress_snapshot.clone(),
                )
                .await;
                warn!(
                    event_name = "openai_image_sync_total_timeout",
                    log_type = "ops",
                    trace_id = %trace_id,
                    request_id = %request_id_for_log,
                    candidate_id = candidate_id.unwrap_or("-"),
                    provider_name,
                    endpoint_id,
                    key_id,
                    model_name,
                    candidate_index,
                    elapsed_ms,
                    timeout_ms,
                    "gateway OpenAI image sync execution exceeded total timeout"
                );
                return Err(SyncExecutionFailure::image_sync_total_timeout(
                    timeout_ms,
                    elapsed_ms,
                ));
            }
        }
    }
}

async fn execute_openai_image_sync_upstream_sse_candidate(
    state: &AppState,
    plan: &ExecutionPlan,
    report_context: Option<&Value>,
    progress_snapshot: Option<Arc<Mutex<OpenAiImageSyncProgressSnapshot>>>,
) -> Result<ExecutionResult, SyncExecutionFailure> {
    let request_body = build_request_body(plan).map_err(SyncExecutionFailure::from_transport)?;
    let started_at = Instant::now();
    let mut progress =
        OpenAiImageSyncProgressRecorder::new(state, plan, report_context, progress_snapshot);
    progress.record_connecting().await;

    let request_started_at_unix_ms = current_request_candidate_unix_ms();
    let request_order_id = uuid::Uuid::now_v7().to_string();
    let response = send_request(plan, request_body)
        .await
        .map_err(SyncExecutionFailure::from_transport)?;
    let ttfb_ms = started_at.elapsed().as_millis() as u64;
    let response_headers_observed_at_unix_ms = current_request_candidate_unix_ms();
    let status_code = response.status_code();
    let headers = response.headers();
    progress.record_response_started(status_code, ttfb_ms).await;

    let mut body_bytes = Vec::new();
    match response {
        DirectHttpResponse::Reqwest(response) => {
            let mut upstream_stream = response.bytes_stream();
            while let Some(chunk) = upstream_stream.next().await {
                let chunk = chunk.map_err(|err| {
                    SyncExecutionFailure::from_transport(
                        ExecutionRuntimeTransportError::UpstreamRequest(
                            format_upstream_request_error(&err),
                        ),
                    )
                })?;
                append_upstream_response_body_chunk(&mut body_bytes, &chunk)
                    .map_err(SyncExecutionFailure::from_transport)?;
                let elapsed_ms = started_at.elapsed().as_millis() as u64;
                progress
                    .observe_chunk(&chunk, status_code, elapsed_ms)
                    .await;
            }
        }
        DirectHttpResponse::HyperH2c(response) => {
            let mut upstream_stream = response.into_body().into_data_stream();
            while let Some(chunk) = upstream_stream.next().await {
                let chunk = chunk.map_err(|err| {
                    SyncExecutionFailure::from_transport(
                        ExecutionRuntimeTransportError::UpstreamRequest(format_hyper_error_chain(
                            &err,
                        )),
                    )
                })?;
                append_upstream_response_body_chunk(&mut body_bytes, &chunk)
                    .map_err(SyncExecutionFailure::from_transport)?;
                let elapsed_ms = started_at.elapsed().as_millis() as u64;
                progress
                    .observe_chunk(&chunk, status_code, elapsed_ms)
                    .await;
            }
        }
        DirectHttpResponse::BrowserWreq(response) => {
            let mut upstream_stream = response.bytes_stream();
            while let Some(chunk) = upstream_stream.next().await {
                let chunk = chunk.map_err(|err| {
                    SyncExecutionFailure::from_transport(
                        ExecutionRuntimeTransportError::UpstreamRequest(
                            format_wreq_upstream_request_error(&err),
                        ),
                    )
                })?;
                append_upstream_response_body_chunk(&mut body_bytes, &chunk)
                    .map_err(SyncExecutionFailure::from_transport)?;
                let elapsed_ms = started_at.elapsed().as_millis() as u64;
                progress
                    .observe_chunk(&chunk, status_code, elapsed_ms)
                    .await;
            }
        }
    }

    let decoded_body_bytes = decode_response_body_bytes(&headers, &body_bytes)
        .map_err(SyncExecutionFailure::from_transport)?;
    let elapsed_ms = started_at.elapsed().as_millis() as u64;
    let upstream_bytes = body_bytes.len() as u64;
    progress.finish(status_code, elapsed_ms).await;

    let body = build_execution_response_body(
        &headers,
        &body_bytes,
        decoded_body_bytes.as_ref(),
        plan.stream,
        execution_response_body_mode(plan),
    )
    .map_err(SyncExecutionFailure::from_transport)?;

    Ok(ExecutionResult {
        request_id: plan.request_id.clone(),
        candidate_id: plan.candidate_id.clone(),
        status_code,
        headers,
        response_observation: Some(ExecutionResponseObservation {
            request_started_at_unix_ms,
            response_headers_observed_at_unix_ms,
            request_order_id,
        }),
        body,
        telemetry: Some(ExecutionTelemetry {
            ttfb_ms: Some(ttfb_ms),
            elapsed_ms: Some(elapsed_ms),
            upstream_bytes: Some(upstream_bytes),
        }),
        error: None,
    })
}

async fn record_openai_image_sync_failed_progress(
    state: &AppState,
    plan: &ExecutionPlan,
    report_context: Option<&Value>,
    status_code: Option<u16>,
    elapsed_ms: u64,
    progress_snapshot: Option<Arc<Mutex<OpenAiImageSyncProgressSnapshot>>>,
) {
    let mut progress =
        OpenAiImageSyncProgressRecorder::new(state, plan, report_context, progress_snapshot);
    progress.fail(status_code, elapsed_ms).await;
}

fn resolve_openai_image_sync_total_timeout_ms(plan: &ExecutionPlan) -> u64 {
    plan.timeouts
        .as_ref()
        .and_then(|timeouts| timeouts.total_ms)
        .unwrap_or(OPENAI_IMAGE_SYNC_DEFAULT_TOTAL_TIMEOUT_MS)
        .max(1)
}

fn report_context_upstream_is_stream(report_context: Option<&Value>) -> bool {
    report_context
        .and_then(|value| value.get(UPSTREAM_IS_STREAM_KEY))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn should_track_openai_image_sync_upstream_sse(
    plan_kind: &str,
    plan: &ExecutionPlan,
    report_context: Option<&Value>,
) -> bool {
    plan_kind == OPENAI_IMAGE_SYNC_PLAN_KIND
        && (plan.stream || report_context_upstream_is_stream(report_context))
}

fn should_enable_openai_image_sync_json_heartbeat(
    _plan_kind: &str,
    _plan: &ExecutionPlan,
    _report_context: Option<&Value>,
) -> bool {
    false
}

#[allow(clippy::too_many_arguments)]
fn build_openai_image_sync_json_heartbeat_response(
    state: AppState,
    request_path: String,
    plan: ExecutionPlan,
    trace_id: String,
    decision: GatewayControlDecision,
    plan_kind: String,
    report_kind: Option<String>,
    report_context: Option<Value>,
) -> Result<Response<Body>, GatewayError> {
    let request_id = plan.request_id.clone();
    let candidate_id = plan.candidate_id.clone();
    let trace_id_for_response = trace_id.clone();
    let decision_for_response = decision.clone();
    let progress_snapshot = Arc::new(Mutex::new(OpenAiImageSyncProgressSnapshot::new()));
    let heartbeat_context = OpenAiImageSyncJsonHeartbeatContext {
        state: state.clone(),
        plan: plan.clone(),
        report_context: report_context.clone(),
        snapshot: progress_snapshot.clone(),
        started_at: Instant::now(),
        trace_id: trace_id.clone(),
        request_id_for_log: short_request_id(request_id.as_str()),
        candidate_id: candidate_id.clone(),
    };
    let (tx, rx) = mpsc::channel::<Result<Bytes, IoError>>(1);

    tokio::spawn(async move {
        let bytes = openai_image_sync_json_heartbeat_final_bytes(
            execute_execution_runtime_sync_impl(
                &state,
                request_path.as_str(),
                plan,
                trace_id.as_str(),
                &decision,
                plan_kind.as_str(),
                report_kind,
                report_context,
                false,
                Some(progress_snapshot),
                None,
                None,
            )
            .await,
        )
        .await;
        let _ = tx.send(Ok(Bytes::from(bytes))).await;
    });

    let headers = BTreeMap::from([(
        CONTENT_TYPE.as_str().to_string(),
        "application/json".to_string(),
    )]);
    let response = build_client_response_from_parts_with_mutator(
        StatusCode::OK.as_u16(),
        &headers,
        Body::from_stream(build_json_whitespace_heartbeat_stream(
            rx,
            OPENAI_IMAGE_SYNC_JSON_HEARTBEAT_INTERVAL,
            Some(heartbeat_context),
        )),
        trace_id_for_response.as_str(),
        Some(&decision_for_response),
        |headers| {
            headers.remove(CONTENT_LENGTH);
            headers.remove(CONTENT_ENCODING);
            headers.insert(
                CACHE_CONTROL,
                HeaderValue::from_static("no-cache, no-transform"),
            );
            headers.insert(
                HeaderName::from_static("x-accel-buffering"),
                HeaderValue::from_static("no"),
            );
            Ok(())
        },
    )?;
    attach_control_metadata_headers(response, Some(request_id.as_str()), candidate_id.as_deref())
}

fn build_json_whitespace_heartbeat_stream(
    mut rx: mpsc::Receiver<Result<Bytes, IoError>>,
    heartbeat_interval: Duration,
    heartbeat_context: Option<OpenAiImageSyncJsonHeartbeatContext>,
) -> impl futures_util::Stream<Item = Result<Bytes, IoError>> + Send + 'static {
    stream! {
        if let Some(context) = heartbeat_context.as_ref() {
            context.record_heartbeat("initial", heartbeat_interval).await;
        }
        yield Ok(Bytes::from_static(OPENAI_IMAGE_SYNC_JSON_HEARTBEAT_BYTES));

        let mut heartbeat = tokio::time::interval(heartbeat_interval);
        heartbeat.set_missed_tick_behavior(MissedTickBehavior::Delay);
        heartbeat.tick().await;
        loop {
            tokio::select! {
                biased;
                item = rx.recv() => {
                    let Some(item) = item else {
                        break;
                    };
                    yield item;
                    break;
                }
                _ = heartbeat.tick() => {
                    if let Some(context) = heartbeat_context.as_ref() {
                        context.record_heartbeat("interval", heartbeat_interval).await;
                    }
                    yield Ok(Bytes::from_static(OPENAI_IMAGE_SYNC_JSON_HEARTBEAT_BYTES));
                }
            }
        }
    }
}

pub(crate) fn build_sync_json_whitespace_heartbeat_stream(
    rx: mpsc::Receiver<Result<Bytes, IoError>>,
) -> impl futures_util::Stream<Item = Result<Bytes, IoError>> + Send + 'static {
    build_json_whitespace_heartbeat_stream(rx, OPENAI_IMAGE_SYNC_JSON_HEARTBEAT_INTERVAL, None)
}

pub(crate) fn build_openai_image_sync_json_whitespace_heartbeat_stream(
    rx: mpsc::Receiver<Result<Bytes, IoError>>,
) -> impl futures_util::Stream<Item = Result<Bytes, IoError>> + Send + 'static {
    build_sync_json_whitespace_heartbeat_stream(rx)
}

async fn openai_image_sync_json_heartbeat_final_bytes(
    result: Result<Option<Response<Body>>, GatewayError>,
) -> Vec<u8> {
    match result {
        Ok(Some(response)) => match to_bytes(
            response.into_body(),
            crate::headers::max_internal_buffered_body_bytes(),
        )
        .await
        {
            Ok(bytes) if !bytes.is_empty() => bytes.to_vec(),
            Ok(_) => openai_image_sync_json_heartbeat_error_body("empty sync image response"),
            Err(err) => openai_image_sync_json_heartbeat_error_body(&err.to_string()),
        },
        Ok(None) => openai_image_sync_json_heartbeat_error_body(
            "sync image execution ended without a local response",
        ),
        Err(err) => openai_image_sync_json_heartbeat_error_body(&format!("{err:?}")),
    }
}

fn openai_image_sync_json_heartbeat_error_body(message: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "error": {
            "type": "aether_gateway_error",
            "message": message,
        }
    }))
    .unwrap_or_else(|_| b"{\"error\":{\"type\":\"aether_gateway_error\"}}".to_vec())
}

async fn apply_sync_success_effects(
    state: &AppState,
    plan: &ExecutionPlan,
    report_context: Option<&serde_json::Value>,
    payload: &GatewaySyncReportRequest,
) {
    apply_local_execution_effect(
        state,
        LocalExecutionEffectContext {
            plan,
            report_context,
        },
        LocalExecutionEffect::HealthSuccess(LocalHealthSuccessEffect),
    )
    .await;
    apply_local_execution_effect(
        state,
        LocalExecutionEffectContext {
            plan,
            report_context,
        },
        LocalExecutionEffect::AdaptiveSuccess(LocalAdaptiveSuccessEffect),
    )
    .await;
}

#[cfg(test)]
enum RemoteSyncFallbackOutcome {
    Executed(ExecutionResult),
    ClientResponse(Response<Body>),
    Unavailable,
}

#[allow(clippy::too_many_arguments)] // internal function, grouping would add unnecessary indirection
pub(crate) async fn execute_execution_runtime_sync(
    state: &AppState,
    request_path: &str,
    mut plan: ExecutionPlan,
    trace_id: &str,
    decision: &GatewayControlDecision,
    plan_kind: &str,
    report_kind: Option<String>,
    mut report_context: Option<serde_json::Value>,
) -> Result<Option<Response<Body>>, GatewayError> {
    execute_execution_runtime_sync_impl(
        state,
        request_path,
        plan,
        trace_id,
        decision,
        plan_kind,
        report_kind,
        report_context,
        true,
        None,
        None,
        None,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn execute_execution_runtime_sync_with_retry_scope(
    state: &AppState,
    request_path: &str,
    plan: ExecutionPlan,
    trace_id: &str,
    decision: &GatewayControlDecision,
    plan_kind: &str,
    report_kind: Option<String>,
    report_context: Option<serde_json::Value>,
) -> Result<AiAttemptExecutionOutcome<Response<Body>>, GatewayError> {
    let mut retry_scope = AiAttemptRetryScope::Candidate;
    let mut fallback_response = None;
    let response = execute_execution_runtime_sync_impl(
        state,
        request_path,
        plan,
        trace_id,
        decision,
        plan_kind,
        report_kind,
        report_context,
        true,
        None,
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
}

#[allow(clippy::too_many_arguments)] // internal function, grouping would add unnecessary indirection
async fn execute_execution_runtime_sync_impl(
    state: &AppState,
    request_path: &str,
    mut plan: ExecutionPlan,
    trace_id: &str,
    decision: &GatewayControlDecision,
    plan_kind: &str,
    report_kind: Option<String>,
    mut report_context: Option<serde_json::Value>,
    allow_json_heartbeat: bool,
    progress_snapshot: Option<Arc<Mutex<OpenAiImageSyncProgressSnapshot>>>,
    mut retry_scope_out: Option<&mut AiAttemptRetryScope>,
    mut retry_fallback_out: Option<&mut Option<Response<Body>>>,
) -> Result<Option<Response<Body>>, GatewayError> {
    if allow_json_heartbeat
        && should_enable_openai_image_sync_json_heartbeat(plan_kind, &plan, report_context.as_ref())
    {
        return build_openai_image_sync_json_heartbeat_response(
            state.clone(),
            request_path.to_string(),
            plan,
            trace_id.to_string(),
            decision.clone(),
            plan_kind.to_string(),
            report_kind,
            report_context,
        )
        .map(Some);
    }

    ensure_execution_request_candidate_slot(state, &mut plan, &mut report_context).await;
    let plan_request_id = plan.request_id.clone();
    let plan_request_id_for_log = short_request_id(plan_request_id.as_str());
    let plan_candidate_id = plan.candidate_id.clone();
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
    let candidate_started_at = Instant::now();
    let candidate_started_unix_secs = current_request_candidate_unix_ms();
    let lifecycle_seed = build_lifecycle_usage_seed(&plan, report_context.as_ref());
    let usage_data = state.usage_lifecycle_data_state().as_ref().clone();
    state
        .usage_runtime
        .record_pending_direct(&usage_data, lifecycle_seed)
        .await;
    record_local_request_candidate_status(
        state,
        &plan,
        report_context.as_ref(),
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
    let mut terminal_guard = SyncAttemptTerminalGuard::new(
        state,
        &plan,
        report_context.clone(),
        candidate_started_unix_secs,
        candidate_started_at,
    );
    let result = (async {
    record_sync_execution_active(
        state,
        &plan,
        report_context.as_ref(),
        candidate_started_unix_secs,
    );
    let mut result = match execute_direct_sync_runtime_candidate(
        state,
        &plan,
        report_context.as_ref(),
        trace_id,
        plan_kind,
        candidate_started_unix_secs,
        plan_request_id_for_log.as_str(),
        plan_candidate_id.as_deref(),
        provider_name.as_str(),
        endpoint_id.as_str(),
        key_id.as_str(),
        model_name.as_str(),
        candidate_index.as_str(),
        progress_snapshot.clone(),
    )
    .await
    {
        Ok(result) => result,
        Err(err) => {
            let failure_error_type = err.error_type;
            let failure_message = err.message.clone();
            let failure_latency_ms = err
                .latency_ms
                .unwrap_or_else(|| elapsed_ms_since(candidate_started_at));
            maybe_store_sync_execution_failure_fallback(
                &err,
                &plan,
                trace_id,
                decision,
                &mut retry_scope_out,
                &mut retry_fallback_out,
            )?;
            warn!(
                event_name = "sync_execution_runtime_unavailable",
                log_type = "ops",
                trace_id = %trace_id,
                request_id = %plan_request_id_for_log,
                candidate_id = ?plan_candidate_id,
                provider_name,
                endpoint_id,
                key_id,
                model_name,
                candidate_index = candidate_index.as_str(),
                error_type = err.error_type,
                error = %err.message,
                "gateway in-process sync execution unavailable"
            );
            let terminal_unix_secs = current_request_candidate_unix_ms();
            record_local_request_candidate_status(
                state,
                &plan,
                report_context.as_ref(),
                SchedulerRequestCandidateStatusUpdate {
                    status: RequestCandidateStatus::Failed,
                    status_code: None,
                    error_type: Some(failure_error_type.to_string()),
                    error_message: Some(err.message),
                    latency_ms: Some(failure_latency_ms),
                    started_at_unix_ms: Some(candidate_started_unix_secs),
                    finished_at_unix_ms: Some(terminal_unix_secs),
                },
            )
            .await;
            if let Some(response) = maybe_build_sync_transport_error_stop_response(
                state,
                &plan,
                report_context.as_ref(),
                trace_id,
                decision,
                failure_error_type,
                failure_message.as_str(),
                failure_latency_ms,
            )
            .await?
            {
                return Ok(Some(response));
            }
            return Ok(None);
        }
    };
    let mut candidate_first_byte_elapsed_ms =
        calibrated_sync_candidate_first_byte_elapsed_ms(candidate_started_at, &result);
    let initial_response_observed_at_unix_ms = current_request_candidate_unix_ms();
    let mut provider_response_observation =
        result
            .response_observation
            .clone()
            .unwrap_or(ExecutionResponseObservation {
                request_started_at_unix_ms: candidate_started_unix_secs,
                response_headers_observed_at_unix_ms: initial_response_observed_at_unix_ms,
                request_order_id: uuid::Uuid::now_v7().to_string(),
            });
    let (
        result_error_type,
        result_error_message,
        result_latency_ms,
        headers,
        body_bytes,
        body_json,
        body_base64,
        local_failover_response_text,
        local_failover_analysis,
    ) = {
        let result_latency_ms = result
            .telemetry
            .as_ref()
            .and_then(|telemetry| telemetry.elapsed_ms);
        let mut headers = std::mem::take(&mut result.headers);
        let (body_bytes, body_json, body_base64) =
            decode_execution_result_body(result.body.take(), &mut headers)?;
        let (mut result_error_type, mut result_error_message) =
            execution_error_details(result.error.as_ref(), body_json.as_ref());
        let local_failover_response_text = local_failover_response_text(
            body_json.as_ref(),
            &body_bytes,
            result.error.as_ref().map(|error| error.message.as_str()),
        );

        let local_failover_analysis = analyze_local_candidate_failover_sync(
            state,
            &plan,
            plan_kind,
            report_context.as_ref(),
            &result,
            local_failover_response_text.as_deref(),
        )
        .await;
        (
            result_error_type,
            result_error_message,
            result_latency_ms,
            headers,
            body_bytes,
            body_json,
            body_base64,
            local_failover_response_text,
            local_failover_analysis,
        )
    };
    let mut report_context = attach_provider_response_headers_to_report_context(
        report_context,
        &headers,
        provider_response_observation.request_started_at_unix_ms,
        provider_response_observation.response_headers_observed_at_unix_ms,
        &provider_response_observation.request_order_id,
    );
    if result.status_code >= 400 {
        apply_local_execution_effect(
            state,
            LocalExecutionEffectContext {
                plan: &plan,
                report_context: report_context.as_ref(),
            },
            LocalExecutionEffect::AttemptFailure(LocalAttemptFailureEffect {
                status_code: result.status_code,
                classification: local_failover_analysis.classification,
            }),
        )
        .await;
        apply_local_execution_effect(
            state,
            LocalExecutionEffectContext {
                plan: &plan,
                report_context: report_context.as_ref(),
            },
            LocalExecutionEffect::AdaptiveRateLimit(LocalAdaptiveRateLimitEffect {
                status_code: result.status_code,
                classification: local_failover_analysis.classification,
                headers: Some(&headers),
            }),
        )
        .await;
        apply_local_execution_effect(
            state,
            LocalExecutionEffectContext {
                plan: &plan,
                report_context: report_context.as_ref(),
            },
            LocalExecutionEffect::HealthFailure(LocalHealthFailureEffect {
                status_code: result.status_code,
                classification: local_failover_analysis.classification,
            }),
        )
        .await;
    }
    if matches!(
        local_failover_analysis.decision,
        LocalFailoverDecision::RetryNextCandidate
    ) {
        let failure_disposition = crate::orchestration::classify_failure_disposition(
            &plan.provider_api_format,
            local_failover_analysis.classification,
            result.status_code,
        );
        if let Some(retry_scope) = retry_scope_out.as_deref_mut() {
            *retry_scope =
                ai_attempt_retry_scope_from_failure_disposition(failure_disposition);
        }
        if failure_disposition.preserve_upstream_error {
            if let Some(retry_fallback) = retry_fallback_out.as_deref_mut() {
                let mut fallback_headers = headers.clone();
                apply_endpoint_response_header_rules(
                    state,
                    &plan,
                    &mut fallback_headers,
                    body_json.as_ref(),
                )
                .await?;
                *retry_fallback = Some(attach_control_metadata_headers(
                    build_client_response_from_parts(
                        result.status_code,
                        &fallback_headers,
                        Body::from(body_bytes.clone()),
                        trace_id,
                        Some(decision),
                    )?,
                    Some(plan.request_id.as_str()),
                    plan.candidate_id.as_deref(),
                )?);
            }
        }
        let terminal_unix_secs = current_request_candidate_unix_ms();
        let error_trace_report_context = with_sync_error_trace_context(
            report_context.as_ref(),
            result.status_code,
            &headers,
            body_json.as_ref(),
            &body_bytes,
            local_failover_response_text.as_deref(),
            local_failover_analysis,
        );
        record_local_request_candidate_status(
            state,
            &plan,
            error_trace_report_context
                .as_ref()
                .or(report_context.as_ref()),
            SchedulerRequestCandidateStatusUpdate {
                status: RequestCandidateStatus::Failed,
                status_code: Some(result.status_code),
                error_type: result_error_type.clone(),
                error_message: result_error_message.clone(),
                latency_ms: result_latency_ms,
                started_at_unix_ms: Some(candidate_started_unix_secs),
                finished_at_unix_ms: Some(terminal_unix_secs),
            },
        )
        .await;
        warn!(
            event_name = "local_sync_candidate_retry_scheduled",
            log_type = "event",
            trace_id = %trace_id,
            request_id = %plan_request_id_for_log,
            status_code = result.status_code,
            provider_name,
            endpoint_id,
            key_id,
            model_name,
            candidate_index = candidate_index.as_str(),
            "gateway local sync decision retrying next candidate after retryable execution runtime result"
        );
        return Ok(None);
    }
    let status_code = result.status_code;
    let has_body_bytes = body_base64.is_some();
    let mut client_headers = headers.clone();
    apply_endpoint_response_header_rules(state, &plan, &mut client_headers, body_json.as_ref())
        .await?;
    let explicit_finalize = should_finalize_sync_response(report_kind.as_deref());
    if !matches!(
        local_failover_analysis.decision,
        LocalFailoverDecision::StopLocalFailover
    ) && should_fallback_to_control_sync(
        plan_kind,
        &result,
        body_json.as_ref(),
        has_body_bytes,
        explicit_finalize,
        false,
    ) {
        let terminal_unix_secs = current_request_candidate_unix_ms();
        let error_trace_report_context = with_sync_error_trace_context(
            report_context.as_ref(),
            result.status_code,
            &headers,
            body_json.as_ref(),
            &body_bytes,
            local_failover_response_text.as_deref(),
            local_failover_analysis,
        );
        record_local_request_candidate_status(
            state,
            &plan,
            error_trace_report_context
                .as_ref()
                .or(report_context.as_ref()),
            SchedulerRequestCandidateStatusUpdate {
                status: RequestCandidateStatus::Failed,
                status_code: Some(result.status_code),
                error_type: result_error_type.clone(),
                error_message: result_error_message.clone(),
                latency_ms: result_latency_ms,
                started_at_unix_ms: Some(candidate_started_unix_secs),
                finished_at_unix_ms: Some(terminal_unix_secs),
            },
        )
        .await;
        return Ok(None);
    }

    let terminal_unix_secs = current_request_candidate_unix_ms();
    let error_flow_report_context = (result.status_code >= 400)
        .then(|| {
            with_sync_error_trace_context(
                report_context.as_ref(),
                result.status_code,
                &headers,
                body_json.as_ref(),
                &body_bytes,
                local_failover_response_text.as_deref(),
                local_failover_analysis,
            )
        })
        .flatten();
    record_local_request_candidate_status(
        state,
        &plan,
        error_flow_report_context
            .as_ref()
            .or(report_context.as_ref()),
        SchedulerRequestCandidateStatusUpdate {
            status: if result.status_code >= 400 {
                RequestCandidateStatus::Failed
            } else {
                RequestCandidateStatus::Success
            },
            status_code: Some(result.status_code),
            error_type: result_error_type.clone(),
            error_message: result_error_message.clone(),
            latency_ms: result_latency_ms,
            started_at_unix_ms: Some(candidate_started_unix_secs),
            finished_at_unix_ms: Some(terminal_unix_secs),
        },
    )
    .await;

    let request_id_owned = result.request_id;
    let candidate_id_owned = result.candidate_id;
    let request_id = (!request_id_owned.trim().is_empty())
        .then_some(request_id_owned.as_str())
        .or(Some(plan_request_id.as_str()));
    let candidate_id = candidate_id_owned
        .as_deref()
        .or(plan_candidate_id.as_deref());
    let report_context = report_context;
    let body_json = body_json;
    let telemetry = result.telemetry;

    let finalize_report_kind = explicit_finalize.then(|| report_kind.clone()).flatten();

    if let Some(finalize_report_kind) = finalize_report_kind {
        let payload = build_sync_report_payload(
            trace_id,
            finalize_report_kind,
            report_context,
            status_code,
            client_headers,
            body_json,
            body_base64,
            telemetry,
        );
        record_sync_terminal_usage_and_disarm_guard(
            state,
            &plan,
            payload.report_context.as_ref(),
            &payload,
            candidate_started_at,
            candidate_first_byte_elapsed_ms,
            &mut terminal_guard,
        )
        .await;
        let response =
            submit_local_core_error_or_sync_finalize(state, trace_id, decision, payload).await?;
        return Ok(Some(attach_control_metadata_headers(
            response,
            request_id,
            candidate_id,
        )?));
    }

    let usage_payload = build_sync_report_payload(
        trace_id,
        report_kind.unwrap_or_default(),
        report_context,
        status_code,
        client_headers,
        body_json,
        body_base64,
        telemetry,
    );
    if status_code < 400 {
        apply_sync_success_effects(
            state,
            &plan,
            usage_payload.report_context.as_ref(),
            &usage_payload,
        )
        .await;
    }
    record_sync_terminal_usage_and_disarm_guard(
        state,
        &plan,
        usage_payload.report_context.as_ref(),
        &usage_payload,
        candidate_started_at,
        candidate_first_byte_elapsed_ms,
        &mut terminal_guard,
    )
    .await;
    let response = attach_control_metadata_headers(
        build_client_response_from_parts(
            status_code,
            &usage_payload.headers,
            Body::from(body_bytes),
            trace_id,
            Some(decision),
        )?,
        request_id,
        candidate_id,
    )?;
    if !usage_payload.report_kind.trim().is_empty() {
        if status_code >= 400 {
            let report_kind = usage_payload.report_kind.clone();
            if let Err(err) = submit_sync_report(state, usage_payload).await {
                warn!(
                    event_name = "local_sync_error_report_submit_failed",
                    log_type = "ops",
                    trace_id = %trace_id,
                    report_kind = %report_kind,
                    "gateway failed to submit local sync error report before returning response: {err:?}"
                );
            }
        } else {
            spawn_sync_report(state.clone(), usage_payload);
        }
    }

    Ok(Some(response))
    })
    .await;
    if let Err(error) = result.as_ref() {
        terminal_guard.fail_and_disarm(error).await;
    } else {
        terminal_guard.disarm();
    }
    result
}

#[allow(clippy::too_many_arguments)] // internal helper mirroring execute path context
#[cfg(test)]
async fn execute_sync_via_remote_execution_runtime(
    state: &AppState,
    remote_execution_runtime_base_url: &str,
    trace_id: &str,
    decision: &GatewayControlDecision,
    plan: &ExecutionPlan,
    plan_request_id: &str,
    plan_candidate_id: Option<&str>,
    report_context: Option<&serde_json::Value>,
    candidate_started_unix_secs: u64,
    candidate_started_at: Instant,
) -> Result<RemoteSyncFallbackOutcome, GatewayError> {
    let remote_request_started_at_unix_ms = current_request_candidate_unix_ms();
    let remote_request_order_id = uuid::Uuid::now_v7().to_string();
    let response = match post_sync_plan_to_remote_execution_runtime(
        state,
        remote_execution_runtime_base_url,
        Some(trace_id),
        plan,
    )
    .await
    {
        Ok(response) => response,
        Err(err) => {
            warn!(
                event_name = "sync_execution_runtime_remote_unavailable",
                log_type = "ops",
                trace_id = %trace_id,
                request_id = %short_request_id(plan_request_id),
                candidate_id = ?plan_candidate_id,
                error = ?err,
                "gateway remote execution runtime sync unavailable"
            );
            let terminal_unix_secs = current_request_candidate_unix_ms();
            record_local_request_candidate_status(
                state,
                plan,
                report_context,
                SchedulerRequestCandidateStatusUpdate {
                    status: RequestCandidateStatus::Failed,
                    status_code: None,
                    error_type: Some("execution_runtime_unavailable".to_string()),
                    error_message: Some(format!("{err:?}")),
                    latency_ms: Some(elapsed_ms_since(candidate_started_at)),
                    started_at_unix_ms: Some(candidate_started_unix_secs),
                    finished_at_unix_ms: Some(terminal_unix_secs),
                },
            )
            .await;
            return Ok(RemoteSyncFallbackOutcome::Unavailable);
        }
    };

    if response.status() != http::StatusCode::OK {
        let terminal_unix_secs = current_request_candidate_unix_ms();
        record_local_request_candidate_status(
            state,
            plan,
            report_context,
            SchedulerRequestCandidateStatusUpdate {
                status: RequestCandidateStatus::Failed,
                status_code: Some(response.status().as_u16()),
                error_type: Some("execution_runtime_http_error".to_string()),
                error_message: Some(format!(
                    "execution runtime returned HTTP {}",
                    response.status()
                )),
                latency_ms: Some(elapsed_ms_since(candidate_started_at)),
                started_at_unix_ms: Some(candidate_started_unix_secs),
                finished_at_unix_ms: Some(terminal_unix_secs),
            },
        )
        .await;
        return Ok(RemoteSyncFallbackOutcome::ClientResponse(
            attach_control_metadata_headers(
                build_client_response(response, trace_id, Some(decision))?,
                Some(plan_request_id),
                plan_candidate_id,
            )?,
        ));
    }

    let remote_response_observed_at_unix_ms = current_request_candidate_unix_ms();
    let mut result = response
        .json::<ExecutionResult>()
        .await
        .map_err(|err| GatewayError::Internal(err.to_string()))?;
    result
        .response_observation
        .get_or_insert(ExecutionResponseObservation {
            request_started_at_unix_ms: remote_request_started_at_unix_ms,
            response_headers_observed_at_unix_ms: remote_response_observed_at_unix_ms,
            request_order_id: remote_request_order_id,
        });
    Ok(RemoteSyncFallbackOutcome::Executed(result))
}
