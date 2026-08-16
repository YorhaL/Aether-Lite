use aether_ai_serving::AiAttemptRetryScope;
use aether_contracts::{
    ExecutionError, ExecutionErrorKind, ExecutionPhase, ExecutionPlan, ExecutionTelemetry,
};
use aether_data_contracts::repository::candidates::RequestCandidateStatus;
use aether_scheduler_core::SchedulerRequestCandidateStatusUpdate;
use aether_usage_runtime::{
    build_sync_terminal_usage_payload_seed, build_terminal_usage_context_seed,
};
use axum::body::Body;
use axum::http::Response;
use base64::Engine as _;
use serde::Serialize;
use serde_json::{Map, Value};
use tracing::warn;

use crate::api::response::attach_control_metadata_headers;
use crate::clock::current_unix_ms as current_request_candidate_unix_ms;
use crate::control::GatewayControlDecision;
use crate::execution_runtime::ai_attempt_retry_scope_from_failure_disposition;
use crate::execution_runtime::submission::{
    resolve_core_error_background_report_kind, submit_local_core_error_or_sync_finalize,
};
use crate::orchestration::{
    apply_local_execution_effect, classify_failure_disposition,
    resolve_local_failover_analysis_for_attempt,
    resolve_local_transport_failover_analysis_for_attempt, with_upstream_response_report_context,
    LocalAdaptiveRateLimitEffect, LocalAttemptFailureEffect, LocalExecutionEffect,
    LocalExecutionEffectContext, LocalFailoverAnalysis, LocalFailoverDecision,
    LocalHealthFailureEffect,
};
use crate::request_candidate_runtime::record_report_request_candidate_status;
use crate::request_diagnostics::attach_current_request_diagnostics_and_candidate_timing_to_report_context;
use crate::usage::submit_sync_report;
use crate::{usage::GatewaySyncReportRequest, AppState, GatewayError};
use aether_gateway_frontdoor::short_request_id;

#[derive(Debug, Clone)]
pub(super) struct StreamFailureReport {
    pub(super) status_code: u16,
    pub(super) error_type: String,
    pub(super) error_message: String,
    upstream_status_code: Option<u16>,
    transport_error: bool,
    honor_http_failover: bool,
    extra_error_fields: Map<String, Value>,
    provider_body_json: Option<Value>,
}

#[derive(Serialize)]
struct StreamFailureBody<'a> {
    error: StreamFailureBodyFields<'a>,
}

#[derive(Serialize)]
struct StreamFailureBodyFields<'a> {
    #[serde(rename = "type")]
    error_type: &'a str,
    message: &'a str,
    code: u16,
    #[serde(flatten)]
    extra_error_fields: &'a Map<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamFailureHandling {
    Terminal,
    HonorLocalFailover,
}

impl StreamFailureReport {
    fn into_body_jsons(self) -> (Value, Option<Value>) {
        let Self {
            status_code,
            error_type,
            error_message,
            upstream_status_code: _,
            transport_error: _,
            honor_http_failover: _,
            mut extra_error_fields,
            provider_body_json,
        } = self;
        extra_error_fields.insert("type".to_string(), Value::String(error_type));
        extra_error_fields.insert("message".to_string(), Value::String(error_message));
        extra_error_fields.insert("code".to_string(), Value::from(status_code));
        let normalized_body = Value::Object(Map::from_iter([(
            "error".to_string(),
            Value::Object(extra_error_fields),
        )]));
        match provider_body_json {
            Some(provider_body) if provider_body != normalized_body => {
                (provider_body, Some(normalized_body))
            }
            Some(provider_body) => (provider_body, None),
            None => (normalized_body, None),
        }
    }

    pub(super) fn to_json_string(&self) -> serde_json::Result<String> {
        serde_json::to_string(&StreamFailureBody {
            error: StreamFailureBodyFields {
                error_type: self.error_type.as_str(),
                message: self.error_message.as_str(),
                code: self.status_code,
                extra_error_fields: &self.extra_error_fields,
            },
        })
    }
}

pub(super) fn build_stream_failure_report(
    error_type: impl Into<String>,
    error_message: impl Into<String>,
    status_code: u16,
) -> StreamFailureReport {
    let error_type = error_type.into();
    let error_message = error_message.into();
    StreamFailureReport {
        status_code,
        error_type,
        error_message,
        upstream_status_code: Some(status_code),
        transport_error: false,
        honor_http_failover: false,
        extra_error_fields: Map::new(),
        provider_body_json: None,
    }
}

pub(super) fn build_stream_transport_failure_report(
    error_type: impl Into<String>,
    error_message: impl Into<String>,
    status_code: u16,
) -> StreamFailureReport {
    StreamFailureReport {
        status_code,
        error_type: error_type.into(),
        error_message: error_message.into(),
        upstream_status_code: None,
        transport_error: true,
        honor_http_failover: false,
        extra_error_fields: Map::new(),
        provider_body_json: None,
    }
}

pub(super) fn build_stream_failure_from_execution_error(
    error: &ExecutionError,
) -> StreamFailureReport {
    let transport_error = execution_error_is_transport(error);
    let fallback_status_code = if matches!(
        error.kind,
        ExecutionErrorKind::ConnectTimeout
            | ExecutionErrorKind::FirstByteTimeout
            | ExecutionErrorKind::ReadTimeout
    ) {
        504
    } else {
        502
    };
    let status_code = error.upstream_status.unwrap_or(fallback_status_code);
    let error_type = serde_json::to_value(&error.kind)
        .ok()
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
        .unwrap_or_else(|| "internal".to_string());
    let error_message = error.message.trim().to_string();
    let phase = serde_json::to_value(&error.phase).unwrap_or(Value::Null);
    let mut error_object = Map::from_iter([
        ("phase".to_string(), phase),
        ("retryable".to_string(), Value::Bool(error.retryable)),
        (
            "failover_recommended".to_string(),
            Value::Bool(error.failover_recommended),
        ),
    ]);
    if let Some(upstream_status) = error.upstream_status {
        error_object.insert("upstream_status".to_string(), Value::from(upstream_status));
    }

    StreamFailureReport {
        status_code,
        error_type,
        error_message,
        upstream_status_code: error.upstream_status,
        transport_error,
        honor_http_failover: error.upstream_status.is_some(),
        extra_error_fields: error_object,
        provider_body_json: None,
    }
}

pub(super) fn build_stream_failure_from_provider_error_body(
    status_code: u16,
    body_json: &Value,
) -> StreamFailureReport {
    let body_object = body_json.as_object();
    let error_object = body_object
        .and_then(|object| object.get("error"))
        .and_then(Value::as_object);
    let error_type =
        first_non_empty_error_text(error_object, body_object, &["type", "code", "status"])
            .unwrap_or_else(|| "upstream_error".to_string());
    let error_message = first_non_empty_error_text(
        error_object,
        body_object,
        &["message", "detail", "reason", "status", "type", "code"],
    )
    .unwrap_or_else(|| format!("upstream stream returned error status {status_code}"));

    StreamFailureReport {
        status_code,
        error_type,
        error_message,
        upstream_status_code: Some(status_code),
        transport_error: false,
        honor_http_failover: true,
        extra_error_fields: Map::new(),
        provider_body_json: Some(body_json.clone()),
    }
}

fn execution_error_is_transport(error: &ExecutionError) -> bool {
    if error.upstream_status.is_some() {
        return false;
    }
    let explicit_transport_kind = matches!(
        error.kind,
        ExecutionErrorKind::ConnectTimeout
            | ExecutionErrorKind::FirstByteTimeout
            | ExecutionErrorKind::ReadTimeout
            | ExecutionErrorKind::TlsError
            | ExecutionErrorKind::ProxyError
            | ExecutionErrorKind::ProtocolError
    );
    let retryable_internal_transport_phase = matches!(error.kind, ExecutionErrorKind::Internal)
        && (error.retryable || error.failover_recommended)
        && matches!(
            error.phase,
            ExecutionPhase::Connect
                | ExecutionPhase::Handshake
                | ExecutionPhase::Write
                | ExecutionPhase::FirstByte
                | ExecutionPhase::StreamRead
        );
    explicit_transport_kind || retryable_internal_transport_phase
}

fn first_non_empty_error_text(
    error_object: Option<&Map<String, Value>>,
    body_object: Option<&Map<String, Value>>,
    keys: &[&str],
) -> Option<String> {
    for object in [error_object, body_object].into_iter().flatten() {
        for key in keys {
            let Some(value) = object.get(*key) else {
                continue;
            };
            match value {
                Value::String(text) if !text.trim().is_empty() => {
                    return Some(text.trim().to_string());
                }
                Value::Number(number) => return Some(number.to_string()),
                _ => {}
            }
        }
    }
    None
}

fn build_stream_failure_sync_payload(
    trace_id: &str,
    report_kind: String,
    report_context: Option<Value>,
    mut headers: std::collections::BTreeMap<String, String>,
    telemetry: Option<ExecutionTelemetry>,
    provider_buffered_body: &[u8],
    failure: StreamFailureReport,
) -> GatewaySyncReportRequest {
    let status_code = failure.status_code;
    let upstream_status_code = failure.upstream_status_code;
    let transport_error = failure.transport_error;
    let (body, client_body) = failure.into_body_jsons();
    headers.retain(|name, _| {
        !name.eq_ignore_ascii_case("content-encoding")
            && !name.eq_ignore_ascii_case("content-length")
            && !name.eq_ignore_ascii_case("content-type")
    });
    headers.insert("content-type".to_string(), "application/json".to_string());
    let report_context = upstream_status_code
        .and_then(|upstream_status_code| {
            with_upstream_response_report_context(
                report_context.as_ref(),
                upstream_status_code,
                Some(&headers),
                Some(&body),
                None,
                None,
            )
        })
        .or(report_context);
    let report_context = report_context.map(|mut context| {
        if let Some(object) = context.as_object_mut() {
            let response_headers = serde_json::to_value(&headers).unwrap_or(Value::Null);
            if upstream_status_code.is_some() {
                object.insert(
                    "provider_response_headers".to_string(),
                    response_headers.clone(),
                );
            }
            object.insert("client_response_headers".to_string(), response_headers);
            if transport_error {
                object.insert("transport_error".to_string(), Value::Bool(true));
            }
        }
        context
    });

    GatewaySyncReportRequest {
        trace_id: trace_id.to_string(),
        report_kind,
        report_context,
        status_code,
        headers,
        body_json: Some(body),
        client_body_json: client_body,
        body_base64: (!provider_buffered_body.is_empty())
            .then(|| base64::engine::general_purpose::STANDARD.encode(provider_buffered_body)),
        telemetry,
    }
}

fn stream_failure_body_field<'a>(
    payload: &'a GatewaySyncReportRequest,
    field: &str,
) -> Option<&'a str> {
    payload
        .client_body_json
        .as_ref()
        .or(payload.body_json.as_ref())
        .and_then(|body_json| body_json.get("error"))
        .and_then(|value| value.get(field))
        .and_then(Value::as_str)
}

async fn record_stream_sync_failure(
    state: &AppState,
    plan: &ExecutionPlan,
    report_context: Option<&Value>,
    payload: &GatewaySyncReportRequest,
    candidate_status_code: Option<u16>,
    started_at_unix_ms: Option<u64>,
    handling: StreamFailureHandling,
) -> LocalFailoverAnalysis {
    let error_type = stream_failure_body_field(payload, "type").unwrap_or("internal");
    let error_message = stream_failure_body_field(payload, "message").unwrap_or_default();
    let error_body = payload
        .body_json
        .as_ref()
        .and_then(|body_json| serde_json::to_string(body_json).ok());
    let failure_analysis = resolve_local_failover_analysis_for_attempt(
        state,
        plan,
        report_context,
        payload.status_code,
        error_body.as_deref(),
    )
    .await;
    apply_local_execution_effect(
        state,
        LocalExecutionEffectContext {
            plan,
            report_context,
        },
        LocalExecutionEffect::AttemptFailure(LocalAttemptFailureEffect {
            status_code: payload.status_code,
            classification: failure_analysis.classification,
        }),
    )
    .await;
    apply_local_execution_effect(
        state,
        LocalExecutionEffectContext {
            plan,
            report_context,
        },
        LocalExecutionEffect::AdaptiveRateLimit(LocalAdaptiveRateLimitEffect {
            status_code: payload.status_code,
            classification: failure_analysis.classification,
            headers: Some(&payload.headers),
        }),
    )
    .await;
    apply_local_execution_effect(
        state,
        LocalExecutionEffectContext {
            plan,
            report_context,
        },
        LocalExecutionEffect::HealthFailure(LocalHealthFailureEffect {
            status_code: payload.status_code,
            classification: failure_analysis.classification,
        }),
    )
    .await;
    let retrying_next_candidate = matches!(
        failure_analysis.decision,
        LocalFailoverDecision::RetryNextCandidate
    );
    if !matches!(handling, StreamFailureHandling::HonorLocalFailover) || !retrying_next_candidate {
        crate::execution_runtime::mark_stream_candidate_watchdog_terminal_started();
        let report_context_with_diagnostics =
            attach_current_request_diagnostics_and_candidate_timing_to_report_context(
                report_context,
                payload
                    .telemetry
                    .as_ref()
                    .and_then(|telemetry| telemetry.elapsed_ms),
                payload
                    .telemetry
                    .as_ref()
                    .and_then(|telemetry| telemetry.ttfb_ms),
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
    let terminal_unix_secs = current_request_candidate_unix_ms();
    record_report_request_candidate_status(
        state,
        report_context,
        SchedulerRequestCandidateStatusUpdate {
            status: RequestCandidateStatus::Failed,
            status_code: candidate_status_code,
            error_type: Some(error_type.to_string()),
            error_message: Some(error_message.to_string()),
            latency_ms: payload
                .telemetry
                .as_ref()
                .and_then(|telemetry| telemetry.elapsed_ms),
            started_at_unix_ms: started_at_unix_ms.or(Some(terminal_unix_secs)),
            finished_at_unix_ms: Some(terminal_unix_secs),
        },
    )
    .await;
    failure_analysis
}

#[allow(clippy::too_many_arguments)] // internal helper for prefetch error handling
pub(super) async fn handle_prefetch_stream_failure(
    state: &AppState,
    trace_id: &str,
    decision: &GatewayControlDecision,
    plan: &ExecutionPlan,
    report_context: Option<Value>,
    request_id: &str,
    candidate_id: Option<&str>,
    report_kind: &str,
    headers: std::collections::BTreeMap<String, String>,
    telemetry: Option<ExecutionTelemetry>,
    buffered_body: &[u8],
    candidate_started_unix_ms: u64,
    candidate_elapsed_ms: u64,
    failure: StreamFailureReport,
    retry_scope_out: Option<&mut AiAttemptRetryScope>,
) -> Result<Option<Response<Body>>, GatewayError> {
    let transport_error = failure.transport_error;
    let candidate_status_code = failure.upstream_status_code;
    let honor_http_failover = failure.honor_http_failover;
    let mut payload = build_stream_failure_sync_payload(
        trace_id,
        report_kind.to_string(),
        report_context,
        headers,
        telemetry,
        buffered_body,
        failure,
    );
    if transport_error {
        let telemetry = payload.telemetry.get_or_insert(ExecutionTelemetry {
            ttfb_ms: None,
            elapsed_ms: None,
            upstream_bytes: None,
        });
        telemetry.elapsed_ms.get_or_insert(candidate_elapsed_ms);
        return handle_prefetch_transport_stream_failure(
            state,
            trace_id,
            decision,
            plan,
            request_id,
            candidate_id,
            payload,
            candidate_started_unix_ms,
            candidate_elapsed_ms,
            retry_scope_out,
        )
        .await;
    }
    let honor_local_failover = honor_http_failover && retry_scope_out.is_some();
    let failure_analysis = record_stream_sync_failure(
        state,
        plan,
        payload.report_context.as_ref(),
        &payload,
        candidate_status_code,
        None,
        if honor_local_failover {
            StreamFailureHandling::HonorLocalFailover
        } else {
            StreamFailureHandling::Terminal
        },
    )
    .await;
    if honor_local_failover
        && matches!(
            failure_analysis.decision,
            LocalFailoverDecision::RetryNextCandidate
        )
    {
        let failure_disposition = classify_failure_disposition(
            &plan.provider_api_format,
            failure_analysis.classification,
            payload.status_code,
        );
        if let Some(retry_scope) = retry_scope_out {
            *retry_scope = ai_attempt_retry_scope_from_failure_disposition(failure_disposition);
        }
        warn!(
            event_name = "local_stream_candidate_retry_scheduled",
            log_type = "event",
            trace_id = %trace_id,
            request_id = %request_id,
            candidate_id = ?candidate_id,
            status_code = payload.status_code,
            failover_classification = failure_analysis.classification.as_str(),
            "gateway local stream decision retrying next candidate after prefetched execution error"
        );
        return Ok(None);
    }

    let response =
        submit_local_core_error_or_sync_finalize(state, trace_id, decision, payload).await?;
    Ok(Some(attach_control_metadata_headers(
        response,
        Some(request_id),
        candidate_id,
    )?))
}

#[allow(clippy::too_many_arguments)]
async fn handle_prefetch_transport_stream_failure(
    state: &AppState,
    trace_id: &str,
    decision: &GatewayControlDecision,
    plan: &ExecutionPlan,
    request_id: &str,
    candidate_id: Option<&str>,
    payload: GatewaySyncReportRequest,
    candidate_started_unix_ms: u64,
    candidate_elapsed_ms: u64,
    retry_scope_out: Option<&mut AiAttemptRetryScope>,
) -> Result<Option<Response<Body>>, GatewayError> {
    let error_type = stream_failure_body_field(&payload, "type").unwrap_or("internal");
    let error_message = stream_failure_body_field(&payload, "message").unwrap_or_default();

    let analysis = resolve_local_transport_failover_analysis_for_attempt(
        state,
        plan,
        payload.report_context.as_ref(),
    )
    .await;
    let retrying_next_candidate = retry_scope_out.is_some()
        && matches!(analysis.decision, LocalFailoverDecision::RetryNextCandidate);
    if !retrying_next_candidate {
        crate::execution_runtime::mark_stream_candidate_watchdog_terminal_started();
        let report_context_with_diagnostics =
            attach_current_request_diagnostics_and_candidate_timing_to_report_context(
                payload.report_context.as_ref(),
                payload
                    .telemetry
                    .as_ref()
                    .and_then(|telemetry| telemetry.elapsed_ms)
                    .or(Some(candidate_elapsed_ms)),
                payload
                    .telemetry
                    .as_ref()
                    .and_then(|telemetry| telemetry.ttfb_ms),
            );
        let context_seed = build_terminal_usage_context_seed(
            plan,
            report_context_with_diagnostics
                .as_ref()
                .or(payload.report_context.as_ref()),
        );
        let payload_seed = build_sync_terminal_usage_payload_seed(&payload);
        state
            .usage_runtime
            .record_sync_terminal(
                state.usage_lifecycle_data_state().as_ref(),
                context_seed,
                payload_seed,
            )
            .await;
    }

    let terminal_unix_ms = current_request_candidate_unix_ms();
    record_report_request_candidate_status(
        state,
        payload.report_context.as_ref(),
        SchedulerRequestCandidateStatusUpdate {
            status: RequestCandidateStatus::Failed,
            status_code: None,
            error_type: Some(error_type.to_string()),
            error_message: Some(error_message.to_string()),
            latency_ms: payload
                .telemetry
                .as_ref()
                .and_then(|telemetry| telemetry.elapsed_ms)
                .or(Some(candidate_elapsed_ms)),
            started_at_unix_ms: Some(candidate_started_unix_ms),
            finished_at_unix_ms: Some(terminal_unix_ms),
        },
    )
    .await;

    if retrying_next_candidate {
        if let Some(retry_scope) = retry_scope_out {
            *retry_scope = AiAttemptRetryScope::Candidate;
        }
        warn!(
            event_name = "local_stream_transport_retry_scheduled",
            log_type = "event",
            trace_id = %trace_id,
            request_id = %request_id,
            candidate_id = ?candidate_id,
            transport_classification = analysis.classification.as_str(),
            "gateway retrying next candidate after precommit transport failure"
        );
        return Ok(None);
    }

    let response =
        submit_local_core_error_or_sync_finalize(state, trace_id, decision, payload).await?;
    Ok(Some(attach_control_metadata_headers(
        response,
        Some(request_id),
        candidate_id,
    )?))
}

pub(super) async fn submit_midstream_stream_failure(
    state: &AppState,
    trace_id: &str,
    plan: &ExecutionPlan,
    direct_stream_finalize_kind: Option<&str>,
    report_context: Option<Value>,
    headers: std::collections::BTreeMap<String, String>,
    telemetry: Option<ExecutionTelemetry>,
    buffered_body: &[u8],
    started_at_unix_ms: u64,
    failure: StreamFailureReport,
) {
    let Some(report_kind) =
        direct_stream_finalize_kind.and_then(resolve_core_error_background_report_kind)
    else {
        return;
    };

    let candidate_status_code = failure.upstream_status_code;
    let payload = build_stream_failure_sync_payload(
        trace_id,
        report_kind,
        report_context,
        headers,
        telemetry,
        buffered_body,
        failure,
    );
    record_stream_sync_failure(
        state,
        plan,
        payload.report_context.as_ref(),
        &payload,
        candidate_status_code,
        Some(started_at_unix_ms),
        StreamFailureHandling::Terminal,
    )
    .await;
    if let Err(err) = submit_sync_report(state, payload).await {
        let request_id = short_request_id(plan.request_id.as_str());
        warn!(
            event_name = "execution_report_submit_failed",
            log_type = "ops",
            trace_id = %trace_id,
            request_id = %request_id,
            candidate_id = ?plan.candidate_id,
            report_scope = "stream_failure",
            error = ?err,
            "gateway failed to submit sync execution report for terminal stream failure"
        );
    }
}
