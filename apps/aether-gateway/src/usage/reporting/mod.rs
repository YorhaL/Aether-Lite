use std::collections::BTreeMap;

use aether_contracts::ExecutionError;
use aether_data_contracts::repository::candidates::RequestCandidateStatus;
use aether_scheduler_core::{execution_error_details, SchedulerRequestCandidateStatusUpdate};
use tracing::{debug, warn};

use crate::clock::current_unix_ms;
use crate::orchestration::{apply_local_report_effect, LocalReportEffect};
use crate::request_candidate_runtime::record_report_request_candidate_status;
use crate::task_runtime::{spawn_fire_and_forget, TASK_KEY_USAGE_SYNC_REPORT};
use crate::{AppState, GatewayError};
use aether_gateway_frontdoor::short_request_id;

mod context;
use context::{report_context_is_locally_actionable, resolve_locally_actionable_report_context};

use aether_usage_runtime::{
    is_local_ai_stream_report_kind, is_local_ai_sync_report_kind, report_request_id,
    should_handle_local_stream_report, should_handle_local_sync_report,
    stream_report_missing_terminal_event, stream_report_represents_failure,
    sync_report_represents_failure, STREAM_MISSING_TERMINAL_EVENT_CATEGORY,
    STREAM_MISSING_TERMINAL_EVENT_MESSAGE, STREAM_TERMINAL_ERROR_CATEGORY,
    STREAM_TERMINAL_ERROR_MESSAGE,
};
pub(crate) use aether_usage_runtime::{GatewayStreamReportRequest, GatewaySyncReportRequest};

fn log_local_report_handled(
    trace_id: &str,
    report_kind: &str,
    report_scope: &'static str,
    report_context: Option<&serde_json::Value>,
) {
    debug!(
        event_name = "execution_report_handled_locally",
        log_type = "debug",
        debug_context = "redacted",
        trace_id = %trace_id,
        report_scope,
        report_kind = %report_kind,
        report_request_id = %short_request_id(report_request_id(report_context)),
        has_report_context = report_context.is_some(),
        "gateway handled execution report locally"
    );
}

fn log_local_report_effect_only(
    trace_id: &str,
    report_kind: &str,
    report_scope: &'static str,
    report_context: Option<&serde_json::Value>,
) {
    debug!(
        event_name = "execution_report_effect_handled_locally",
        log_type = "debug",
        debug_context = "redacted",
        trace_id = %trace_id,
        report_scope,
        report_kind = %report_kind,
        report_request_id = %short_request_id(report_request_id(report_context)),
        has_report_context = report_context.is_some(),
        "gateway handled execution report locally without actionable request-candidate context"
    );
}

fn log_dropped_report(
    trace_id: &str,
    report_kind: &str,
    report_scope: &'static str,
    report_context: Option<&serde_json::Value>,
) {
    warn!(
        event_name = "execution_report_dropped",
        log_type = "ops",
        status = "dropped",
        trace_id = %trace_id,
        report_scope,
        report_kind = %report_kind,
        report_request_id = %short_request_id(report_request_id(report_context)),
        has_report_context = report_context.is_some(),
        "gateway dropped execution report because local handling context was not actionable"
    );
}

pub(crate) async fn submit_sync_report(
    state: &AppState,
    mut payload: GatewaySyncReportRequest,
) -> Result<(), GatewayError> {
    let original_report_context = payload.report_context.take();
    if let Some(report_context) =
        resolve_locally_actionable_report_context(state, original_report_context.as_ref()).await
    {
        payload.report_context = Some(report_context);
        if should_handle_local_sync_report(
            payload.report_context.as_ref(),
            payload.report_kind.as_str(),
        ) {
            handle_local_sync_report(state, &payload).await;
            log_local_report_handled(
                payload.trace_id.as_str(),
                &payload.report_kind,
                "sync",
                payload.report_context.as_ref(),
            );
            return Ok(());
        }
    }
    payload.report_context = original_report_context;

    if should_handle_local_sync_report(
        payload.report_context.as_ref(),
        payload.report_kind.as_str(),
    ) {
        handle_local_sync_report(state, &payload).await;
        log_local_report_handled(
            payload.trace_id.as_str(),
            &payload.report_kind,
            "sync",
            payload.report_context.as_ref(),
        );
        return Ok(());
    }

    if payload.report_context.is_some()
        && is_local_ai_sync_report_kind(payload.report_kind.as_str())
    {
        handle_local_sync_report(state, &payload).await;
        log_local_report_effect_only(
            payload.trace_id.as_str(),
            &payload.report_kind,
            "sync",
            payload.report_context.as_ref(),
        );
        return Ok(());
    }

    log_dropped_report(
        payload.trace_id.as_str(),
        &payload.report_kind,
        "sync",
        payload.report_context.as_ref(),
    );
    Ok(())
}

pub(crate) fn spawn_sync_report(state: AppState, payload: GatewaySyncReportRequest) {
    let report_request_id_for_log =
        short_request_id(report_request_id(payload.report_context.as_ref()));
    spawn_fire_and_forget(TASK_KEY_USAGE_SYNC_REPORT, async move {
        let trace_id = payload.trace_id.clone();
        if let Err(err) = submit_sync_report(&state, payload).await {
            warn!(
                event_name = "execution_report_submit_failed",
                log_type = "ops",
                trace_id = %trace_id,
                report_scope = "sync",
                report_request_id = %report_request_id_for_log,
                error = ?err,
                "gateway failed to submit sync execution report"
            );
        }
    });
}

pub(crate) async fn submit_stream_report(
    state: &AppState,
    mut payload: GatewayStreamReportRequest,
) -> Result<(), GatewayError> {
    let original_report_context = payload.report_context.take();
    if let Some(report_context) =
        resolve_locally_actionable_report_context(state, original_report_context.as_ref()).await
    {
        payload.report_context = Some(report_context);
        if should_handle_local_stream_report(
            payload.report_context.as_ref(),
            payload.report_kind.as_str(),
        ) {
            handle_local_stream_report(state, &payload).await;
            log_local_report_handled(
                payload.trace_id.as_str(),
                &payload.report_kind,
                "stream",
                payload.report_context.as_ref(),
            );
            return Ok(());
        }
    }
    payload.report_context = original_report_context;

    if should_handle_local_stream_report(
        payload.report_context.as_ref(),
        payload.report_kind.as_str(),
    ) {
        handle_local_stream_report(state, &payload).await;
        log_local_report_handled(
            payload.trace_id.as_str(),
            &payload.report_kind,
            "stream",
            payload.report_context.as_ref(),
        );
        return Ok(());
    }

    if payload.report_context.is_some()
        && is_local_ai_stream_report_kind(payload.report_kind.as_str())
    {
        handle_local_stream_report(state, &payload).await;
        log_local_report_effect_only(
            payload.trace_id.as_str(),
            &payload.report_kind,
            "stream",
            payload.report_context.as_ref(),
        );
        return Ok(());
    }

    log_dropped_report(
        payload.trace_id.as_str(),
        &payload.report_kind,
        "stream",
        payload.report_context.as_ref(),
    );
    Ok(())
}

async fn handle_local_sync_report(state: &AppState, payload: &GatewaySyncReportRequest) {
    let terminal_unix_ms = current_unix_ms();
    let (error_type, error_message) =
        execution_error_details(None::<&ExecutionError>, payload.body_json.as_ref());
    let status = if sync_report_represents_failure(payload, error_type.as_deref()) {
        RequestCandidateStatus::Failed
    } else {
        RequestCandidateStatus::Success
    };
    let latency_ms = payload
        .telemetry
        .as_ref()
        .and_then(|telemetry| telemetry.elapsed_ms);
    record_report_request_candidate_status(
        state,
        payload.report_context.as_ref(),
        SchedulerRequestCandidateStatusUpdate {
            status,
            status_code: Some(payload.status_code),
            error_type,
            error_message,
            latency_ms,
            started_at_unix_ms: None,
            finished_at_unix_ms: Some(terminal_unix_ms),
        },
    )
    .await;
    apply_local_report_effect(state, LocalReportEffect::Sync { payload }).await;
}

async fn handle_local_stream_report(state: &AppState, payload: &GatewayStreamReportRequest) {
    let terminal_unix_ms = current_unix_ms();
    let latency_ms = payload
        .telemetry
        .as_ref()
        .and_then(|telemetry| telemetry.elapsed_ms);
    let failed = stream_report_represents_failure(payload);
    let missing_terminal_event = stream_report_missing_terminal_event(payload);
    record_report_request_candidate_status(
        state,
        payload.report_context.as_ref(),
        SchedulerRequestCandidateStatusUpdate {
            status: if failed {
                RequestCandidateStatus::Failed
            } else {
                RequestCandidateStatus::Success
            },
            status_code: Some(payload.status_code),
            error_type: failed.then(|| {
                if payload.status_code >= 400 {
                    "stream_http_error".to_string()
                } else if missing_terminal_event {
                    STREAM_MISSING_TERMINAL_EVENT_CATEGORY.to_string()
                } else {
                    STREAM_TERMINAL_ERROR_CATEGORY.to_string()
                }
            }),
            error_message: failed.then(|| {
                payload
                    .terminal_summary
                    .as_ref()
                    .and_then(|summary| summary.parser_error.clone())
                    .unwrap_or_else(|| {
                        if missing_terminal_event {
                            STREAM_MISSING_TERMINAL_EVENT_MESSAGE.to_string()
                        } else {
                            STREAM_TERMINAL_ERROR_MESSAGE.to_string()
                        }
                    })
            }),
            latency_ms,
            started_at_unix_ms: None,
            finished_at_unix_ms: Some(terminal_unix_ms),
        },
    )
    .await;
    apply_local_report_effect(state, LocalReportEffect::Stream { payload }).await;
}
